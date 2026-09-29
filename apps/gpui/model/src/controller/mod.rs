//! O controlador: porte das regras do `PlayerViewModel.swift`, sem nada de UI.
//!
//! Só quem chama `tick()` (a main, no app) mexe no modelo. Os comandos bloqueantes rodam em
//! série numa thread própria, e o volume e as cargas têm threads próprias. As respostas voltam
//! por canal e são aplicadas no tique seguinte, como a main actor fazia no Swift.

mod commands;
mod loaders;
mod model;
mod volume;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use hog_audio::api::Snapshot;
use hog_audio::transitions::PlayerState;

use crate::display::{clock_state, display_state};
use crate::entry::{QueueEntryInfo, load_queue_entry};
use crate::media::tags::file_stem;
use crate::metadata::{TrackMetadata, load_track_metadata};
use crate::port::{Format, Player, Track};
use commands::{Effect, Envelope, Job, JobKind, Outcome, Shared};
use loaders::{EntryLoader, MetadataLoader};
use model::Observed;
use volume::{VolumeResult, VolumeWorker};

pub use model::{Changes, Model, QueueState, VolumeState};

pub type EntryLoad = Arc<dyn Fn(&str) -> QueueEntryInfo + Send + Sync>;
pub type MetadataLoad = Arc<dyn Fn(&str) -> TrackMetadata + Send + Sync>;
pub type LogSink = Arc<dyn Fn(&str) + Send + Sync>;
/// Chamado por qualquer thread do controlador ao entregar uma resposta. O app acorda a main para
/// um tique na hora, como o `apply` do Swift, que chamava `refresh()` sem esperar o timer.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// A mesma tolerância da conferência de escrita do engine: abaixo dela é o volume que nós mesmos
/// pedimos, só que ida e volta por `f32`.
const VOLUME_TOLERANCE: f64 = 0.005;
const INITIAL_VOLUME: f64 = 0.5;

pub struct Config {
    pub load_entry: EntryLoad,
    pub load_metadata: MetadataLoad,
    pub log: LogSink,
    pub wake: Wake,
}

impl Config {
    pub fn production(log: LogSink, wake: Wake) -> Self {
        Self {
            load_entry: Arc::new(load_queue_entry),
            load_metadata: Arc::new(load_track_metadata),
            log,
            wake,
        }
    }
}

pub struct Controller {
    player: Arc<dyn Player>,
    shared: Arc<Shared>,
    jobs: Sender<Envelope>,
    outcomes: Receiver<Outcome>,
    volume: VolumeWorker,
    entries: EntryLoader,
    metadata: MetadataLoader,
    log: LogSink,
    model: Model,
    format: Option<Format>,
    /// Os metadados completos e de qual faixa eles são.
    current_metadata: Option<TrackMetadata>,
    metadata_path: Option<String>,
    reload_metadata: bool,
    last_queue_version: u64,
    last_problem_seq: u64,
    in_flight: usize,
    advancing: bool,
    advanced_finished_index: Option<u32>,
    recovering: bool,
    volume_editing: bool,
    shutting_down: bool,
}

impl Controller {
    pub fn new(player: Arc<dyn Player>, config: Config) -> Self {
        let snapshot = player.snapshot();
        let items: Vec<Track> = player.queue_items().into_iter().map(Track::from).collect();
        let shared = Arc::new(Shared::new(snapshot.problem_seq));
        let wake = config.wake;
        let (jobs, outcomes) =
            commands::spawn(Arc::clone(&player), Arc::clone(&shared), Arc::clone(&wake));
        let mut controller = Self {
            volume: VolumeWorker::spawn(Arc::clone(&player), Arc::clone(&wake)),
            entries: EntryLoader::spawn(config.load_entry, Arc::clone(&wake)),
            metadata: MetadataLoader::new(config.load_metadata, wake),
            log: config.log,
            model: Model {
                display: display_state(&snapshot, None, None),
                clock: clock_state(&snapshot),
                queue: QueueState {
                    items,
                    current_index: snapshot.current_index,
                    is_playing: snapshot.state == PlayerState::Playing,
                },
                entries: HashMap::new(),
                volume: VolumeState {
                    value: INITIAL_VOLUME,
                    muted: false,
                },
                error: None,
            },
            format: None,
            current_metadata: None,
            metadata_path: None,
            reload_metadata: false,
            last_queue_version: snapshot.queue_version,
            // Um problema de antes de a janela abrir não é notícia.
            last_problem_seq: snapshot.problem_seq,
            in_flight: 0,
            advancing: false,
            advanced_finished_index: None,
            recovering: false,
            volume_editing: false,
            shutting_down: false,
            player,
            shared,
            jobs,
            outcomes,
        };
        controller
            .entries
            .sync(&controller.model.queue.items, &mut controller.model.entries);
        controller
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// O poll: dez vezes por segundo no app. Aplica as respostas, lê o snapshot (só atômicos no
    /// core) e dispara recuperação e auto-avanço. Devolve quais partes do modelo mudaram.
    pub fn tick(&mut self) -> Changes {
        let before = Observed::capture(&self.model);
        let mut items_changed = false;

        while let Ok(outcome) = self.outcomes.try_recv() {
            self.apply(outcome);
        }
        self.apply_volume_results();
        let mut entries_changed = self.entries.drain(&mut self.model.entries);

        let snapshot = self.player.snapshot();
        self.model.queue.current_index = snapshot.current_index;
        // `try_queue_items` nunca espera o engine: com um comando segurando o lock (uma troca de
        // device leva segundos), a versão não avança e o próximo tique tenta de novo.
        if self.last_queue_version != snapshot.queue_version
            && let Some(items) = self.player.try_queue_items()
        {
            let items: Vec<Track> = items.into_iter().map(Track::from).collect();
            if items != self.model.queue.items {
                self.model.queue.items = items;
                items_changed = true;
            }
            self.last_queue_version = snapshot.queue_version;
            entries_changed |= self
                .entries
                .sync(&self.model.queue.items, &mut self.model.entries);
        }
        if snapshot.current_index.is_none() {
            self.forget_current_track();
        }
        // Com a fila da tela defasada (outro comando segurando o engine), a faixa atual dela pode
        // não ser a do engine: o pedido e o resultado dos metadados esperam, no canal, o tique em
        // que a fila sincronizar.
        let queue_in_sync = self.last_queue_version == snapshot.queue_version;
        if queue_in_sync {
            self.accept_metadata();
            if std::mem::take(&mut self.reload_metadata) {
                self.reload_metadata_for_current_track();
            }
        }

        self.model.queue.is_playing = snapshot.state == PlayerState::Playing;
        let shown = self.shown_metadata();
        self.model.display = display_state(&snapshot, shown.as_ref(), self.format.as_ref());
        self.model.clock = clock_state(&snapshot);
        self.model.volume.muted = snapshot.muted;
        self.follow_device_volume(&snapshot);
        self.show_new_problem(&snapshot);

        // Antes do auto-avanço: a recuperação conta como comando em curso, e é isso que impede
        // tocar a faixa seguinte no device que o macOS pôs no lugar do perdido.
        if snapshot.device_suspect && !self.recovering && !self.shutting_down {
            self.recovering = true;
            self.enqueue(Job::Recover);
        }
        if snapshot.state != PlayerState::Finished {
            self.advanced_finished_index = None;
        }
        if let Some(index) = self.finished_index_to_advance(&snapshot) {
            self.advancing = true;
            self.advanced_finished_index = Some(index);
            self.enqueue(Job::Advance);
        }

        before.changes(&self.model, items_changed, entries_changed)
    }

    pub fn toggle(&mut self) {
        self.enqueue(Job::Toggle);
    }

    pub fn next(&mut self) {
        self.navigate(1);
    }

    pub fn previous(&mut self) {
        self.navigate(-1);
    }

    /// Por id, não por índice: a fila da tela pode estar defasada em relação à do engine, e o
    /// índice de uma linha muda a cada remoção.
    pub fn select(&mut self, id: u64) {
        self.enqueue(Job::Select(id));
    }

    pub fn remove(&mut self, id: u64) {
        self.enqueue(Job::Remove(id));
    }

    pub fn clear(&mut self) {
        self.enqueue(Job::Clear);
    }

    /// Expandir pastas roda na thread de comandos: uma pasta grande leva segundos.
    pub fn add(&mut self, paths: Vec<PathBuf>) {
        self.enqueue(Job::Add(paths));
    }

    /// Publica na hora: o slider não pode esperar o mutex do core, o mesmo que um play segura
    /// durante a aquisição do hog.
    pub fn apply_volume(&mut self, scalar: f64) {
        if self.shutting_down {
            return;
        }
        self.model.volume.value = scalar;
        self.volume.request(scalar as f32);
    }

    pub fn set_volume_editing(&mut self, editing: bool) {
        self.volume_editing = editing;
    }

    /// Descarta o que ainda não começou e devolve o device. O `shutdown` do core espera o
    /// comando em curso pelo lock de comandos; a falha volta para quem chamou registrar, porque
    /// a janela já está fechando.
    pub fn shutdown(&mut self) -> Result<(), String> {
        if self.shutting_down {
            return Ok(());
        }
        self.shutting_down = true;
        self.shared.shutting_down.store(true, Ordering::Release);
        self.volume.stop();
        self.entries.stop();
        self.metadata.cancel();
        self.player.shutdown().map_err(|error| error.to_string())
    }

    pub fn is_idle(&self) -> bool {
        self.in_flight == 0 && !self.volume.busy()
    }

    pub fn wait_idle(&mut self, timeout: Duration) -> bool {
        self.wait_until(timeout, Self::is_idle)
    }

    pub fn wait_entries(&mut self, timeout: Duration) -> bool {
        self.wait_until(timeout, |c| c.entries.idle())
    }

    pub fn wait_metadata(&mut self, timeout: Duration) -> bool {
        self.wait_until(timeout, |c| c.metadata.idle())
    }

    /// Tique até `done` valer, com teto. Existe para os testes e para o encerramento; a UI normal
    /// só chama `tick()`.
    pub fn wait_until(&mut self, timeout: Duration, done: impl Fn(&Self) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            self.tick();
            if done(self) {
                self.tick();
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Cinco cliques rápidos em Next eram cinco cargas completas, cada uma soltando e tomando o
    /// device. Enquanto a navegação ainda espera a vez, cliques novos só somam passos nela.
    fn navigate(&mut self, delta: i32) {
        if self.shutting_down {
            return;
        }
        {
            let mut pending = self.shared.navigation();
            if let Some(steps) = pending.as_mut() {
                *steps += delta;
                return;
            }
            *pending = Some(delta);
        }
        self.enqueue(Job::Navigate);
    }

    fn enqueue(&mut self, job: Job) {
        if self.shutting_down {
            return;
        }
        self.in_flight += 1;
        if self
            .jobs
            .send(Envelope {
                job,
                queued_at: Instant::now(),
            })
            .is_err()
        {
            self.in_flight -= 1;
        }
    }

    fn apply(&mut self, outcome: Outcome) {
        self.in_flight = self.in_flight.saturating_sub(1);
        match outcome.kind {
            JobKind::Advance => self.advancing = false,
            JobKind::Recover => self.recovering = false,
            _ => {}
        }
        let at_start = outcome
            .report
            .as_ref()
            .map(|report| report.problem_seq_at_start);
        if let Some(report) = &outcome.report {
            (self.log)(&report.line());
        }
        match outcome.effect {
            Effect::Nothing | Effect::Advanced(Ok(None)) => {}
            Effect::Toggled {
                result,
                device_volume,
            } => {
                match result {
                    Ok(()) => self.clear_error_from_before(at_start),
                    Err(message) => self.model.error = Some(message),
                }
                if let Some(scalar) = device_volume {
                    self.model.volume.value = f64::from(scalar);
                }
            }
            Effect::Loaded(Ok(format))
            | Effect::Advanced(Ok(Some(format)))
            | Effect::Removed(Ok(Some(format))) => {
                self.format = Some(format);
                self.clear_error_from_before(at_start);
                // Não espera os metadados: o título aparece na hora pela linha da fila e a capa
                // chega quando a leitura terminar.
                self.reload_metadata = true;
            }
            Effect::Removed(Ok(None)) => self.clear_error_from_before(at_start),
            Effect::Cleared(Ok(())) => {
                self.forget_current_track();
                self.model.error = None;
            }
            Effect::Loaded(Err(message))
            | Effect::Advanced(Err(message))
            | Effect::Removed(Err(message))
            | Effect::Cleared(Err(message))
            | Effect::AddFailed(message) => self.model.error = Some(message),
        }
    }

    /// Um comando bem-sucedido limpa o erro de antes dele, mas não um problema que o engine
    /// publicou durante o próprio comando e um tique já mostrou.
    fn clear_error_from_before(&mut self, at_start: Option<u64>) {
        if at_start.is_none_or(|seq| seq == self.last_problem_seq) {
            self.model.error = None;
        }
    }

    fn apply_volume_results(&mut self) {
        for result in self.volume.drain() {
            match result {
                VolumeResult::Discarded => {}
                VolumeResult::Written => self.model.error = None,
                VolumeResult::Failed {
                    message,
                    device_volume,
                } => {
                    // O slider não pode continuar exibindo um pedido que o device recusou.
                    self.model.error = Some(message);
                    if let Some(scalar) = device_volume {
                        self.model.volume.value = f64::from(scalar);
                    }
                }
            }
        }
    }

    /// O slider segue o volume mudado por fora só quando nada nosso mexe nele. Fora de Playing e
    /// Paused, o device está sendo trocado ou já não é nosso. Durante um arrasto, uma escrita ou
    /// um comando, o hardware ainda tem o valor de antes.
    fn follow_device_volume(&mut self, snapshot: &Snapshot) {
        let Some(scalar) = snapshot.volume_scalar else {
            return;
        };
        let ours = matches!(snapshot.state, PlayerState::Playing | PlayerState::Paused);
        if !ours || self.volume_editing || self.volume.busy() || self.in_flight > 0 {
            return;
        }
        let device = f64::from(scalar);
        if (device - self.model.volume.value).abs() > VOLUME_TOLERANCE {
            self.model.volume.value = device;
        }
    }

    fn show_new_problem(&mut self, snapshot: &Snapshot) {
        if snapshot.problem_seq == self.last_problem_seq {
            return;
        }
        self.last_problem_seq = snapshot.problem_seq;
        self.shared
            .shown_problem_seq
            .store(snapshot.problem_seq, Ordering::Release);
        if let Some(problem) = self.player.last_problem() {
            self.model.error = Some(problem);
        }
    }

    fn finished_index_to_advance(&self, snapshot: &Snapshot) -> Option<u32> {
        if self.shutting_down
            || self.advancing
            || self.in_flight > 0
            || snapshot.state != PlayerState::Finished
        {
            return None;
        }
        let current = snapshot.current_index?;
        (current + 1 < snapshot.queue_len && self.advanced_finished_index != Some(current))
            .then_some(current)
    }

    /// Outra faixa pode ter assumido enquanto esta lia: o resultado dela não vale mais.
    fn accept_metadata(&mut self) {
        let Some((path, metadata)) = self.metadata.drain() else {
            return;
        };
        if self
            .model
            .queue
            .current()
            .is_some_and(|track| track.path == path)
        {
            self.current_metadata = Some(metadata);
            self.metadata_path = Some(path);
        }
    }

    fn reload_metadata_for_current_track(&mut self) {
        let Some(path) = self.model.queue.current().map(|track| track.path.clone()) else {
            self.forget_current_track();
            return;
        };
        // Previous que rebobina recarrega a mesma faixa: os metadados já são dela.
        if self.metadata_path.as_deref() == Some(path.as_str()) {
            return;
        }
        if let Err(error) = self.metadata.request(path) {
            (self.log)(&format!(
                "não consegui criar a thread de metadados: {error}"
            ));
        }
    }

    /// Sem faixa atual não há o que mostrar: sem isto a tela ficaria com a capa e o formato da
    /// faixa removida.
    fn forget_current_track(&mut self) {
        self.current_metadata = None;
        self.metadata_path = None;
        self.format = None;
        self.metadata.cancel();
    }

    /// Logo depois de uma troca os metadados completos ainda não chegaram. O título vem da linha
    /// da fila (ou do nome do arquivo), e a capa da faixa anterior fica até a nova chegar, em vez
    /// de piscar o placeholder.
    fn shown_metadata(&self) -> Option<TrackMetadata> {
        let Some(track) = self.model.queue.current() else {
            return self.current_metadata.clone();
        };
        if self.metadata_path.as_deref() == Some(track.path.as_str()) {
            return self.current_metadata.clone();
        }
        let entry = self.model.entries.get(&track.path);
        Some(TrackMetadata {
            title: entry.map_or_else(|| file_stem(&track.path), |e| e.title.clone()),
            artist: entry.and_then(|e| e.artist.clone()),
            album: entry.and_then(|e| e.album.clone()),
            artwork: self
                .current_metadata
                .as_ref()
                .and_then(|m| m.artwork.clone()),
        })
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.entries.stop();
        self.volume.stop();
    }
}
