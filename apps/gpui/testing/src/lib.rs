//! O dublê do `HogPlayer` e os helpers dos testes: porte do `FakePlayer` de
//! `ViewModelTests.swift`. Crate só de teste, para o `src/` dos outros crates ficar só com código
//! de produção. Registra o que foi chamado e deixa o teste segurar comandos com portões.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hog_audio::api::{PlayerError, QueueItem, Snapshot, TrackFormat, TransitionTimings};
use hog_audio::transitions::PlayerState;
use hog_player_model::controller::{Config, Controller, LogSink};
use hog_player_model::entry::QueueEntryInfo;
use hog_player_model::metadata::TrackMetadata;
use hog_player_model::port::Player;

pub const PRAZO: Duration = Duration::from_secs(2);

/// Segura uma chamada até o teste abrir: prova ordem e descarte sem depender de relógio.
#[derive(Default)]
pub struct Gate {
    open: Mutex<bool>,
    wake: Condvar,
}

impl Gate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn wait(&self) {
        let mut open = self.open.lock().unwrap_or_else(|p| p.into_inner());
        while !*open {
            open = self.wake.wait(open).unwrap_or_else(|p| p.into_inner());
        }
    }

    pub fn open(&self) {
        *self.open.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.wake.notify_all();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u64,
    pub path: String,
    pub failure: Option<String>,
}

/// Ids sequenciais a partir de 1, como o engine dá a uma fila nova.
pub fn itens(paths: &[&str]) -> Vec<Item> {
    paths
        .iter()
        .enumerate()
        .map(|(i, path)| Item {
            id: i as u64 + 1,
            path: path.to_string(),
            failure: None,
        })
        .collect()
}

pub struct FakeState {
    pub chamadas: Vec<String>,
    pub estado: PlayerState,
    pub volume_aplicado: Option<f32>,
    /// O volume que o device "de verdade" tem: é o que o snapshot devolve.
    pub volume_real: Option<f32>,
    pub mudo: bool,
    pub falha_ao_aplicar_volume: Option<String>,
    /// O atraso vale só para o escalar indicado, para identificar a escrita certa.
    pub atraso_de_volume: Duration,
    pub escalar_com_atraso: Option<f32>,
    pub escritas_de_volume: usize,
    /// Segura a escrita de volume em curso, e `volume_esperando` diz que ela já chegou ao dublê.
    pub portao_do_volume: Option<Arc<Gate>>,
    pub volume_esperando: bool,
    /// Depois do `shutdown`, o dublê recusa o volume como o core e conta quem ainda tentou.
    pub encerrado: bool,
    pub volume_depois_do_shutdown: usize,
    pub itens: Vec<Item>,
    pub indice_atual: Option<u32>,
    pub versao_fila: u64,
    pub leituras_da_fila: usize,
    pub avancos: usize,
    pub atraso_de_avanco: Duration,
    pub atraso_de_selecao: Duration,
    pub portao_da_selecao: Option<Arc<Gate>>,
    pub selecao_esperando: bool,
    pub decorrido: f64,
    pub suspeita_de_device: bool,
    pub seq_problema: u64,
    pub problema: Option<String>,
    pub recuperacoes: usize,
    /// O aviso que o engine publica no fim de um Play, e o portão que segura o `play()` depois
    /// de publicá-lo: reproduz um tique exibindo o aviso antes de o comando voltar.
    pub aviso_ao_tocar: Option<String>,
    pub portao_do_play: Option<Arc<Gate>>,
    /// O device recusando o play, como o `PlayerError::Device` do core.
    pub falha_ao_tocar: Option<String>,
    /// O engine ocupado numa troca de device: `try_queue_items` desiste em vez de esperar.
    pub fila_ocupada: bool,
    pub navegacoes: Vec<i32>,
}

impl Default for FakeState {
    fn default() -> Self {
        Self {
            chamadas: Vec::new(),
            estado: PlayerState::Idle,
            volume_aplicado: None,
            volume_real: Some(0.5),
            mudo: false,
            falha_ao_aplicar_volume: None,
            atraso_de_volume: Duration::ZERO,
            escalar_com_atraso: None,
            escritas_de_volume: 0,
            portao_do_volume: None,
            volume_esperando: false,
            encerrado: false,
            volume_depois_do_shutdown: 0,
            itens: Vec::new(),
            indice_atual: None,
            versao_fila: 0,
            leituras_da_fila: 0,
            avancos: 0,
            atraso_de_avanco: Duration::ZERO,
            atraso_de_selecao: Duration::ZERO,
            portao_da_selecao: None,
            selecao_esperando: false,
            decorrido: 0.0,
            suspeita_de_device: false,
            seq_problema: 0,
            problema: None,
            recuperacoes: 0,
            aviso_ao_tocar: None,
            portao_do_play: None,
            falha_ao_tocar: None,
            fila_ocupada: false,
            navegacoes: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct FakePlayer {
    state: Mutex<FakeState>,
}

impl FakePlayer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut FakeState) -> R) -> R {
        f(&mut self.lock())
    }

    /// Acrescenta sem mexer na versão da fila: o teste decide quando o controlador relê.
    pub fn acrescentar(&self, path: &str) {
        let mut s = self.lock();
        let item = novo_item(&s.itens, path);
        s.itens.push(item);
    }

    pub fn chamou(&self, nome: &str) -> bool {
        self.lock().chamadas.iter().any(|c| c == nome)
    }

    /// Espera o dublê chegar a um estado, com teto; devolve se chegou.
    pub fn esperar(&self, prazo: Duration, condicao: impl Fn(&FakeState) -> bool) -> bool {
        let limite = Instant::now() + prazo;
        while Instant::now() < limite {
            if condicao(&self.lock()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }
}

fn novo_item(itens: &[Item], path: &str) -> Item {
    let id = itens.iter().map(|i| i.id).max().unwrap_or(0) + 1;
    Item {
        id,
        path: path.to_string(),
        failure: None,
    }
}

fn track() -> TrackFormat {
    TrackFormat {
        sample_rate: 44_100.0,
        bit_depth: 16,
        channels: 2,
        codec: "flac".to_string(),
        device_name: "Fake".to_string(),
        total_seconds: 10.0,
    }
}

fn state_error(message: &str) -> PlayerError {
    PlayerError::State {
        message: message.to_string(),
    }
}

fn queue_items(itens: &[Item]) -> Vec<QueueItem> {
    itens
        .iter()
        .map(|i| QueueItem {
            id: i.id,
            path: i.path.clone(),
            failure: i.failure.clone(),
        })
        .collect()
}

impl Player for FakePlayer {
    fn append_tracks(&self, paths: Vec<String>) -> u32 {
        let mut s = self.lock();
        for path in &paths {
            let item = novo_item(&s.itens, path);
            s.itens.push(item);
        }
        if s.indice_atual.is_none() && !s.itens.is_empty() {
            s.indice_atual = Some(0);
            s.estado = PlayerState::Loaded;
        }
        if !paths.is_empty() {
            s.versao_fila += 1;
        }
        paths.len() as u32
    }

    fn queue_items(&self) -> Vec<QueueItem> {
        let mut s = self.lock();
        s.leituras_da_fila += 1;
        queue_items(&s.itens)
    }

    fn try_queue_items(&self) -> Option<Vec<QueueItem>> {
        let mut s = self.lock();
        if s.fila_ocupada {
            return None;
        }
        s.leituras_da_fila += 1;
        Some(queue_items(&s.itens))
    }

    fn navigate(&self, steps: i32) -> Result<TrackFormat, PlayerError> {
        let mut s = self.lock();
        s.navegacoes.push(steps);
        let Some(current) = s.indice_atual.filter(|_| !s.itens.is_empty()) else {
            return Err(state_error("sem faixa"));
        };
        let alvo = (i64::from(current) + i64::from(steps)).clamp(0, s.itens.len() as i64 - 1);
        s.indice_atual = Some(alvo as u32);
        if s.estado != PlayerState::Playing {
            s.estado = PlayerState::Loaded;
        }
        Ok(track())
    }

    fn select_track(&self, index: u32) -> Result<TrackFormat, PlayerError> {
        let (atraso, portao) = {
            let mut s = self.lock();
            s.selecao_esperando = true;
            (s.atraso_de_selecao, s.portao_da_selecao.clone())
        };
        std::thread::sleep(atraso);
        if let Some(portao) = portao {
            portao.wait();
        }
        let mut s = self.lock();
        if index as usize >= s.itens.len() {
            return Err(state_error("índice inválido"));
        }
        s.chamadas.push("select".to_string());
        s.indice_atual = Some(index);
        if s.estado != PlayerState::Playing {
            s.estado = PlayerState::Loaded;
        }
        Ok(track())
    }

    fn select_track_id(&self, id: u64) -> Result<TrackFormat, PlayerError> {
        let index = self.lock().itens.iter().position(|i| i.id == id);
        match index {
            Some(index) => self.select_track(index as u32),
            None => Err(state_error("a faixa não está mais na fila")),
        }
    }

    fn advance(&self) -> Result<Option<TrackFormat>, PlayerError> {
        let atraso = self.lock().atraso_de_avanco;
        std::thread::sleep(atraso);
        let mut s = self.lock();
        s.avancos += 1;
        let Some(current) = s.indice_atual else {
            return Ok(None);
        };
        if current as usize + 1 >= s.itens.len() {
            return Ok(None);
        }
        s.indice_atual = Some(current + 1);
        s.estado = PlayerState::Playing;
        Ok(Some(track()))
    }

    /// Como o engine: removida a atual, quem assume a posição é carregado aqui mesmo e o
    /// formato dela volta na resposta.
    fn remove_track_id(&self, id: u64) -> Result<Option<TrackFormat>, PlayerError> {
        let mut s = self.lock();
        let Some(index) = s.itens.iter().position(|i| i.id == id) else {
            return Err(state_error("a faixa não está mais na fila"));
        };
        s.chamadas.push("remove".to_string());
        let before = s.indice_atual.map(|i| i as usize);
        s.itens.remove(index);
        s.versao_fila += 1;
        if before == Some(index) {
            if index >= s.itens.len() {
                s.indice_atual = None;
                s.estado = PlayerState::Idle;
                return Ok(None);
            }
            s.indice_atual = Some(index as u32);
            return Ok(Some(track()));
        }
        if let Some(before) = before
            && index < before
        {
            s.indice_atual = Some(before as u32 - 1);
        }
        Ok(None)
    }

    fn clear_queue(&self) -> Result<(), PlayerError> {
        let mut s = self.lock();
        s.itens.clear();
        s.indice_atual = None;
        s.estado = PlayerState::Idle;
        s.versao_fila += 1;
        Ok(())
    }

    fn play(&self) -> Result<(), PlayerError> {
        let portao = {
            let mut s = self.lock();
            s.chamadas.push("play".to_string());
            if let Some(message) = s.falha_ao_tocar.clone() {
                return Err(PlayerError::Device { message });
            }
            s.estado = PlayerState::Playing;
            if let Some(aviso) = s.aviso_ao_tocar.clone() {
                s.problema = Some(aviso);
                s.seq_problema += 1;
            }
            s.portao_do_play.clone()
        };
        if let Some(portao) = portao {
            portao.wait();
        }
        Ok(())
    }

    fn pause(&self) -> Result<(), PlayerError> {
        let mut s = self.lock();
        s.chamadas.push("pause".to_string());
        s.estado = PlayerState::Paused;
        Ok(())
    }

    fn set_volume(&self, scalar: f32) -> Result<(), PlayerError> {
        let (atraso, alvo, portao) = {
            let mut s = self.lock();
            if s.encerrado {
                s.volume_depois_do_shutdown += 1;
                return Err(state_error("o player está encerrando"));
            }
            s.volume_esperando = true;
            (
                s.atraso_de_volume,
                s.escalar_com_atraso,
                s.portao_do_volume.clone(),
            )
        };
        if alvo == Some(scalar) {
            std::thread::sleep(atraso);
        }
        if let Some(portao) = portao {
            portao.wait();
        }
        let mut s = self.lock();
        s.escritas_de_volume += 1;
        if let Some(message) = s.falha_ao_aplicar_volume.clone() {
            return Err(PlayerError::Device { message });
        }
        s.volume_aplicado = Some(scalar);
        s.volume_real = Some(scalar);
        Ok(())
    }

    fn snapshot(&self) -> Snapshot {
        let s = self.lock();
        Snapshot {
            state: s.estado,
            elapsed_seconds: s.decorrido,
            total_seconds: 10.0,
            underruns: 0,
            volume_scalar: s.volume_real,
            muted: s.mudo,
            current_index: s.indice_atual,
            queue_len: s.itens.len() as u32,
            queue_version: s.versao_fila,
            device_suspect: s.suspeita_de_device,
            problem_seq: s.seq_problema,
        }
    }

    fn recover(&self) -> Result<bool, PlayerError> {
        let mut s = self.lock();
        s.chamadas.push("recover".to_string());
        s.recuperacoes += 1;
        if !s.suspeita_de_device {
            return Ok(false);
        }
        s.suspeita_de_device = false;
        s.estado = PlayerState::Loaded;
        s.problema = Some("o device de saída foi desconectado; a reprodução parou".to_string());
        s.seq_problema += 1;
        Ok(true)
    }

    fn last_problem(&self) -> Option<String> {
        self.lock().problema.clone()
    }

    fn last_timings(&self) -> TransitionTimings {
        TransitionTimings {
            open_ms: 0.0,
            release_ms: 0.0,
            reopen_ms: 0.0,
            acquire_ms: 0.0,
            volume_ms: 0.0,
            prefill_ms: 0.0,
            start_ms: 0.0,
        }
    }

    fn shutdown(&self) -> Result<(), PlayerError> {
        let mut s = self.lock();
        s.chamadas.push("shutdown".to_string());
        s.encerrado = true;
        Ok(())
    }
}

pub fn entrada(title: &str) -> QueueEntryInfo {
    QueueEntryInfo {
        title: title.to_string(),
        artist: None,
        album: None,
        quality: None,
        quality_detail: None,
        duration: None,
        thumbnail: None,
        problem: None,
    }
}

pub fn metadados(title: &str) -> TrackMetadata {
    TrackMetadata {
        title: title.to_string(),
        artist: None,
        album: None,
        artwork: None,
    }
}

pub fn sem_log() -> LogSink {
    Arc::new(|_: &str| {})
}

/// Cargas instantâneas: a linha vira "fila <caminho>" e os metadados, o próprio caminho. Nos
/// testes quem chama `tick()` é o próprio teste, então acordar não faz nada.
pub fn config_de_teste() -> Config {
    Config {
        load_entry: Arc::new(|path: &str| entrada(&format!("fila {path}"))),
        load_metadata: Arc::new(|path: &str| metadados(path)),
        log: sem_log(),
        wake: Arc::new(|| {}),
    }
}

pub fn controlador(fake: &Arc<FakePlayer>) -> Controller {
    Controller::new(fake.clone(), config_de_teste())
}

/// Uma pasta temporária nova, com arquivos vazios dos nomes pedidos.
pub fn pasta_com(nome: &str, arquivos: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("hog-gpui-{nome}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("pasta temporária");
    for arquivo in arquivos {
        std::fs::write(root.join(arquivo), b"").expect("arquivo temporário");
    }
    root
}
