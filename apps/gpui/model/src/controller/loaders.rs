//! As cargas fora da main: as linhas da fila, em quatro threads, e os metadados completos da
//! faixa atual, fora da cadeia de comandos.

use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use super::{EntryLoad, MetadataLoad, Wake};
use crate::entry::QueueEntryInfo;
use crate::media::tags::file_stem;
use crate::metadata::TrackMetadata;
use crate::port::Track;

/// Soltar uma pasta de mil faixas não pode disparar mil leituras de tag e de capa ao mesmo
/// tempo: quatro enchem a lista de cima para baixo sem disputar CPU e disco com a reprodução.
const ENTRY_WORKERS: usize = 4;

#[derive(Default)]
struct Pending {
    paths: VecDeque<String>,
    stop: bool,
}

#[derive(Default)]
struct EntryShared {
    pending: Mutex<Pending>,
    wake: Condvar,
}

impl EntryShared {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(|p| p.into_inner())
    }
}

pub(crate) struct EntryLoader {
    shared: Arc<EntryShared>,
    results: Receiver<(String, QueueEntryInfo)>,
    /// Pedidos que ainda não voltaram: na espera, carregando ou com o resultado no canal.
    requested: HashSet<String>,
}

impl EntryLoader {
    pub fn spawn(load: EntryLoad, wake: Wake) -> Self {
        let shared = Arc::new(EntryShared::default());
        let (tx, results) = mpsc::channel();
        for index in 0..ENTRY_WORKERS {
            let (shared, load, tx) = (Arc::clone(&shared), Arc::clone(&load), tx.clone());
            let wake = Arc::clone(&wake);
            std::thread::Builder::new()
                .name(format!("hog-entry-{index}"))
                .spawn(move || {
                    while let Some(path) = next_path(&shared) {
                        let info = load_entry_guarded(&load, &path);
                        if tx.send((path, info)).is_err() {
                            return;
                        }
                        wake();
                    }
                })
                .expect("não consegui criar a thread de carga");
        }
        Self {
            shared,
            results,
            requested: HashSet::new(),
        }
    }

    /// Alinha a carga com a fila: descarta o que saiu e pede, na ordem da fila, o que falta. O
    /// topo visível enche primeiro. Devolve se alguma linha saiu.
    pub fn sync(&mut self, queue: &[Track], entries: &mut HashMap<String, QueueEntryInfo>) -> bool {
        let queued: HashSet<&str> = queue.iter().map(|track| track.path.as_str()).collect();
        let before = entries.len();
        entries.retain(|path, _| queued.contains(path.as_str()));
        self.requested.retain(|path| queued.contains(path.as_str()));
        let mut pending = self.shared.lock();
        pending.paths.retain(|path| queued.contains(path.as_str()));
        for track in queue {
            if !entries.contains_key(&track.path) && self.requested.insert(track.path.clone()) {
                pending.paths.push_back(track.path.clone());
            }
        }
        drop(pending);
        self.shared.wake.notify_all();
        entries.len() != before
    }

    /// A faixa pode ter saído da fila enquanto carregava: só vale o que ainda está pedido.
    pub fn drain(&mut self, entries: &mut HashMap<String, QueueEntryInfo>) -> bool {
        let mut changed = false;
        while let Ok((path, info)) = self.results.try_recv() {
            if self.requested.remove(&path) {
                entries.insert(path, info);
                changed = true;
            }
        }
        changed
    }

    pub fn idle(&self) -> bool {
        self.requested.is_empty()
    }

    pub fn stop(&mut self) {
        let mut pending = self.shared.lock();
        pending.stop = true;
        pending.paths.clear();
        drop(pending);
        self.shared.wake.notify_all();
        self.requested.clear();
    }
}

/// Uma capa corrompida pode fazer o decodificador entrar em pânico. A thread de carga não pode
/// morrer com ela: o caminho ficaria pedido para sempre, e quatro pânicos parariam todas as linhas.
fn load_entry_guarded(load: &EntryLoad, path: &str) -> QueueEntryInfo {
    catch_unwind(AssertUnwindSafe(|| load(path))).unwrap_or_else(|_| unreadable_entry(path))
}

fn unreadable_entry(path: &str) -> QueueEntryInfo {
    const UNREADABLE: &str = "não consegui ler o arquivo";
    QueueEntryInfo {
        title: file_stem(path),
        artist: None,
        album: None,
        quality: None,
        quality_detail: None,
        duration: None,
        thumbnail: None,
        problem: Some(UNREADABLE.to_string()),
    }
}

fn next_path(shared: &EntryShared) -> Option<String> {
    let mut pending = shared.lock();
    loop {
        if pending.stop {
            return None;
        }
        if let Some(path) = pending.paths.pop_front() {
            return Some(path);
        }
        pending = shared.wake.wait(pending).unwrap_or_else(|p| p.into_inner());
    }
}

type MetadataResult = (u64, String, TrackMetadata);

/// Os metadados completos da faixa atual, com a capa grande. Cada pedido tem uma geração; o
/// resultado de uma geração antiga é descartado, porque outra faixa assumiu enquanto ele lia.
pub(crate) struct MetadataLoader {
    load: MetadataLoad,
    wake: Wake,
    generation: u64,
    tx: Sender<MetadataResult>,
    results: Receiver<MetadataResult>,
    running: Arc<AtomicUsize>,
}

impl MetadataLoader {
    pub fn new(load: MetadataLoad, wake: Wake) -> Self {
        let (tx, results) = mpsc::channel();
        Self {
            load,
            wake,
            generation: 0,
            tx,
            results,
            running: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Roda dentro do `tick`: a falha ao criar a thread volta para quem chamou registrar, em vez
    /// de derrubar a main.
    pub fn request(&mut self, path: String) -> std::io::Result<()> {
        self.generation += 1;
        let generation = self.generation;
        let (load, tx, running) = (
            Arc::clone(&self.load),
            self.tx.clone(),
            Arc::clone(&self.running),
        );
        let wake = Arc::clone(&self.wake);
        self.running.fetch_add(1, Ordering::AcqRel);
        let spawned = std::thread::Builder::new()
            .name("hog-metadata".to_string())
            .spawn(move || {
                // Uma capa corrompida pode fazer a leitura entrar em pânico: a faixa fica sem
                // metadados novos, mas o contador volta, ou a espera ficaria presa.
                if let Ok(metadata) = catch_unwind(AssertUnwindSafe(|| load(&path))) {
                    // Envia antes de baixar o contador: quando `idle()` vale, o resultado já está
                    // no canal.
                    let _ = tx.send((generation, path, metadata));
                }
                running.fetch_sub(1, Ordering::AcqRel);
                wake();
            });
        if spawned.is_err() {
            self.running.fetch_sub(1, Ordering::AcqRel);
        }
        spawned.map(|_| ())
    }

    pub fn cancel(&mut self) {
        self.generation += 1;
    }

    pub fn drain(&mut self) -> Option<(String, TrackMetadata)> {
        let mut latest = None;
        while let Ok((generation, path, metadata)) = self.results.try_recv() {
            if generation == self.generation {
                latest = Some((path, metadata));
            }
        }
        latest
    }

    pub fn idle(&self) -> bool {
        self.running.load(Ordering::Acquire) == 0
    }
}
