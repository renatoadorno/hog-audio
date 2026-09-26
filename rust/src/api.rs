//! A superfície que o Swift enxerga. Só controle e leitura de estado atravessam esta
//! fronteira: nenhum dado de áudio passa por aqui, e o IOProc nunca a alcança.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::engine::{Engine, LoadFailure, LoadedTrack, PlayFailure};
use crate::queue::{QueueAction, next_playable, on_navigate, on_next, on_previous};
use crate::source::AudioSource;
use crate::transitions::PlayerState;
use crate::volume::{VolumeRequest, VolumeUnit, parse_volume};

#[derive(uniffi::Record)]
pub struct Snapshot {
    pub state: PlayerState,
    pub elapsed_seconds: f64,
    pub total_seconds: f64,
    pub underruns: u64,
    pub volume_scalar: f32,
    pub current_index: Option<u32>,
    pub queue_len: u32,
    pub queue_version: u64,
}

#[derive(uniffi::Record)]
pub struct QueueItem {
    pub path: String,
    pub failure: Option<String>,
}

#[derive(uniffi::Record)]
pub struct TrackFormat {
    pub sample_rate: f64,
    pub bit_depth: u32,
    pub channels: u32,
    pub codec: String,
    pub device_name: String,
    pub total_seconds: f64,
}

#[derive(uniffi::Record)]
pub struct TrackProbe {
    pub sample_rate: f64,
    pub bit_depth: u32,
    pub channels: u32,
    pub codec: String,
    pub total_seconds: f64,
}

/// Quanto cada etapa do último comando de transporte levou, em milissegundos. `reopen` é a
/// segunda abertura do arquivo, a que alimenta a produtora.
#[derive(uniffi::Record)]
pub struct TransitionTimings {
    pub open_ms: f64,
    pub release_ms: f64,
    pub reopen_ms: f64,
    pub acquire_ms: f64,
    pub volume_ms: f64,
    pub prefill_ms: f64,
    pub start_ms: f64,
}

#[derive(uniffi::Error, Debug)]
pub enum PlayerError {
    Load { message: String },
    Device { message: String },
    State { message: String },
}

impl std::fmt::Display for PlayerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayerError::Load { message }
            | PlayerError::Device { message }
            | PlayerError::State { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for PlayerError {}

#[derive(uniffi::Object)]
pub struct HogPlayer {
    engine: Engine,
    commands: Mutex<()>,
    shutting_down: AtomicBool,
}

#[uniffi::export]
impl HogPlayer {
    /// Cria um player novo, em repouso (`Idle`) e sem device nenhum tomado — só `load` toca no
    /// hardware.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            engine: Engine::new(),
            commands: Mutex::new(()),
            shutting_down: AtomicBool::new(false),
        })
    }

    pub fn append_tracks(&self, paths: Vec<String>) -> u32 {
        let _command = self.command_lock();
        if self.shutting_down.load(Ordering::Acquire) {
            return 0;
        }
        let was_empty = self.engine.queue_position().1 == 0;
        let count = self.engine.append_tracks(paths) as u32;
        if was_empty && count > 0 {
            let _ = self.load_queue_index(0, false, true);
        }
        count
    }

    pub fn queue_items(&self) -> Vec<QueueItem> {
        self.engine
            .queue_entries()
            .into_iter()
            .map(|entry| QueueItem {
                path: entry.path,
                failure: entry.failure,
            })
            .collect()
    }

    /// Como `queue_items`, mas nunca espera: `None` enquanto um comando segura o engine. É o
    /// que a interface usa no poll, para a janela não congelar durante uma troca de device.
    pub fn try_queue_items(&self) -> Option<Vec<QueueItem>> {
        self.engine.try_queue_entries().map(|entries| {
            entries
                .into_iter()
                .map(|entry| QueueItem {
                    path: entry.path,
                    failure: entry.failure,
                })
                .collect()
        })
    }

    /// Next/Previous somados: `steps` positivo avança, negativo volta, com a regra de
    /// rebobinar valendo só para o primeiro passo para trás. Carrega uma faixa só, por mais
    /// cliques que a interface tenha juntado.
    pub fn navigate(&self, steps: i32) -> Result<TrackFormat, PlayerError> {
        let _command = self.command_lock();
        self.ensure_running()?;
        self.engine.reset_timings();
        let (current, len) = self.engine.queue_position();
        let elapsed = self.engine.status().elapsed_seconds();
        let action = on_navigate(current, len, elapsed, steps).ok_or_else(|| {
            let message = if steps > 0 {
                "já está na última faixa"
            } else if steps < 0 {
                "já está no início da fila"
            } else {
                "nenhum passo para navegar"
            };
            PlayerError::State {
                message: message.to_string(),
            }
        })?;
        self.execute_manual_action(action)
    }

    pub fn select_track(&self, index: u32) -> Result<TrackFormat, PlayerError> {
        let _command = self.command_lock();
        self.ensure_running()?;
        self.engine.reset_timings();
        let was_playing = self.engine.state() == PlayerState::Playing;
        self.load_queue_index(index as usize, was_playing, true)
    }

    pub fn next_track(&self) -> Result<TrackFormat, PlayerError> {
        let _command = self.command_lock();
        self.ensure_running()?;
        self.engine.reset_timings();
        let (current, len) = self.engine.queue_position();
        let action = on_next(current, len).ok_or_else(|| PlayerError::State {
            message: "já está na última faixa".to_string(),
        })?;
        self.execute_manual_action(action)
    }

    pub fn previous_track(&self) -> Result<TrackFormat, PlayerError> {
        let _command = self.command_lock();
        self.ensure_running()?;
        self.engine.reset_timings();
        let (current, _) = self.engine.queue_position();
        let action =
            on_previous(current, self.engine.status().elapsed_seconds()).ok_or_else(|| {
                PlayerError::State {
                    message: "já está no início da fila".to_string(),
                }
            })?;
        self.execute_manual_action(action)
    }

    pub fn advance(&self) -> Result<Option<TrackFormat>, PlayerError> {
        let _command = self.command_lock();
        self.ensure_running()?;
        self.engine.reset_timings();
        let (current, len) = self.engine.queue_position();
        let Some(current) = current else {
            return Ok(None);
        };
        let mut from = current.saturating_add(1);
        loop {
            let failed = self.engine.failed_tracks();
            let Some(index) = next_playable(from, &failed, len) else {
                return Ok(None);
            };
            match self.load_queue_index(index, true, false) {
                Ok(track) => return Ok(Some(track)),
                Err(PlayerError::Load { .. }) => from = index.saturating_add(1),
                Err(error) => return Err(error),
            }
        }
    }

    pub fn remove_track(&self, index: u32) -> Result<(), PlayerError> {
        let _command = self.command_lock();
        self.ensure_running()?;
        self.engine.reset_timings();
        let was_playing = self.engine.state() == PlayerState::Playing;
        let (reload, current) = self
            .engine
            .remove_queue_entry(index as usize)
            .map_err(|message| PlayerError::State { message })?;
        if reload {
            match current {
                Some(current) => {
                    self.load_queue_index(current, was_playing, true)?;
                }
                None => self
                    .engine
                    .shutdown()
                    .map_err(|message| PlayerError::Device { message })?,
            }
        }
        Ok(())
    }

    pub fn clear_queue(&self) -> Result<(), PlayerError> {
        let _command = self.command_lock();
        self.ensure_running()?;
        let result = self
            .engine
            .shutdown()
            .map_err(|message| PlayerError::Device { message });
        self.engine.clear_queue_entries();
        result
    }

    /// Começa ou retoma a reprodução. As duas causas de falha chegam com tipos diferentes de
    /// propósito: `PlayerError::State` quando o pedido não faz sentido no estado atual (nada
    /// carregado, ou uma falha anterior que só se resolve recarregando) — nada que reter o
    /// hardware; `PlayerError::Device` quando o device recusou ou não respondeu.
    pub fn play(&self) -> Result<(), PlayerError> {
        let _command = self.command_lock();
        self.ensure_running()?;
        self.engine.reset_timings();
        match self.engine.play_classified() {
            Ok(()) => Ok(()),
            Err(PlayFailure::State(message)) => Err(PlayerError::State { message }),
            Err(PlayFailure::Track(message)) => {
                if let Some(index) = self.engine.queue_position().0 {
                    self.engine.mark_queue_failure(index, Some(message.clone()));
                }
                Err(PlayerError::Load { message })
            }
            Err(PlayFailure::Device(message)) => Err(PlayerError::Device { message }),
        }
    }

    /// Suspende a reprodução sem soltar o device — retomar depois com `play` é rápido porque o
    /// hog mode continua ativo. Só existe uma causa de falha aqui, a transição de estado, então
    /// chega sempre como `PlayerError::State`.
    pub fn pause(&self) -> Result<(), PlayerError> {
        let _command = self.command_lock();
        self.ensure_running()?;
        self.engine
            .pause()
            .map_err(|message| PlayerError::State { message })
    }

    /// Aplica o volume direto no device, nunca em software sobre as amostras — é assim que a
    /// reprodução continua bit-perfect. `scalar` é grampeado em 0..1 antes de chegar ao
    /// hardware, então o Swift não precisa validar o próprio input.
    pub fn set_volume(&self, scalar: f32) -> Result<(), PlayerError> {
        let _command = self.command_lock();
        self.ensure_running()?;
        let scalar = scalar.clamp(0.0, 1.0);
        self.engine
            .set_volume(scalar)
            .map_err(|message| PlayerError::Device { message })
    }

    /// Só lê atômicos, então nunca bloqueia — mesmo com um `play` lento segurando o mutex do
    /// engine na aquisição do hog. É o que permite a interface consultar dez vezes por
    /// segundo sem travar.
    pub fn snapshot(&self) -> Snapshot {
        self.engine.poll_finished();
        let status = self.engine.status();
        Snapshot {
            state: status.state(),
            elapsed_seconds: status.elapsed_seconds(),
            total_seconds: status.total_seconds(),
            underruns: status.underruns(),
            volume_scalar: status.volume(),
            current_index: status.current_index(),
            queue_len: status.queue_len(),
            queue_version: status.queue_version(),
        }
    }

    /// Não pega o lock de comandos: a medição tem mutex próprio, então dá para ler logo depois
    /// de um comando sem esperar o seguinte.
    pub fn last_timings(&self) -> TransitionTimings {
        let timings = self.engine.timings();
        TransitionTimings {
            open_ms: timings.open_ms,
            release_ms: timings.release_ms,
            reopen_ms: timings.reopen_ms,
            acquire_ms: timings.acquire_ms,
            volume_ms: timings.volume_ms,
            prefill_ms: timings.prefill_ms,
            start_ms: timings.start_ms,
        }
    }

    /// Solta o device e devolve-o ao estado original — sample rate, formato e volume. Propaga
    /// a falha de restauração em vez de engolir: sem isso o app fecharia e o Mac podia ficar
    /// com outro sample rate sem ninguém saber.
    pub fn shutdown(&self) -> Result<(), PlayerError> {
        self.shutting_down.store(true, Ordering::Release);
        let _command = self.command_lock();
        self.engine
            .shutdown()
            .map_err(|message| PlayerError::Device { message })
    }
}

impl HogPlayer {
    fn command_lock(&self) -> MutexGuard<'_, ()> {
        self.commands
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn ensure_running(&self) -> Result<(), PlayerError> {
        if self.shutting_down.load(Ordering::Acquire) {
            Err(PlayerError::State {
                message: "o player está encerrando".to_string(),
            })
        } else {
            Ok(())
        }
    }

    fn execute_manual_action(&self, action: QueueAction) -> Result<TrackFormat, PlayerError> {
        let was_playing = self.engine.state() == PlayerState::Playing;
        let index = match action {
            QueueAction::Load(index) => index,
            QueueAction::Restart => {
                self.engine
                    .queue_position()
                    .0
                    .ok_or_else(|| PlayerError::State {
                        message: "nenhuma faixa selecionada".to_string(),
                    })?
            }
            QueueAction::Stop => {
                return Err(PlayerError::State {
                    message: "fim da fila".to_string(),
                });
            }
        };
        self.load_queue_index(index, was_playing, true)
    }

    fn load_queue_index(
        &self,
        index: usize,
        should_play: bool,
        stop_on_track_failure: bool,
    ) -> Result<TrackFormat, PlayerError> {
        let path = self
            .engine
            .queue_path(index)
            .ok_or_else(|| PlayerError::State {
                message: "índice da fila fora do intervalo".to_string(),
            })?;
        let loaded = match self.engine.load_classified(&path) {
            Ok(track) => track,
            Err(LoadFailure::Track(message)) => {
                self.engine.mark_queue_failure(index, Some(message.clone()));
                if stop_on_track_failure {
                    let _ = self.engine.shutdown();
                    self.engine.set_current_index(Some(index));
                }
                return Err(PlayerError::Load { message });
            }
            Err(LoadFailure::Device(message)) => {
                return Err(PlayerError::Device { message });
            }
        };

        self.engine.set_current_index(Some(index));
        self.engine.mark_queue_failure(index, None);
        if should_play {
            match self.engine.play_classified() {
                Ok(()) => {}
                Err(PlayFailure::Track(message)) => {
                    self.engine.mark_queue_failure(index, Some(message.clone()));
                    if stop_on_track_failure {
                        let _ = self.engine.shutdown();
                        self.engine.set_current_index(Some(index));
                    }
                    return Err(PlayerError::Load { message });
                }
                Err(PlayFailure::State(message)) => {
                    return Err(PlayerError::State { message });
                }
                Err(PlayFailure::Device(message)) => {
                    return Err(PlayerError::Device { message });
                }
            }
        }
        Ok(track_format(loaded))
    }
}

fn track_format(track: LoadedTrack) -> TrackFormat {
    TrackFormat {
        sample_rate: track.sample_rate,
        bit_depth: track.bit_depth,
        channels: track.channels,
        codec: track.codec,
        device_name: track.device_name,
        total_seconds: track.total_seconds,
    }
}

/// O formato de um arquivo, lido sem carregá-lo no player. Função livre de propósito: não
/// passa pelo `Engine` nem pelo lock de comandos, nunca toca no device e pode rodar em
/// paralelo — é o que a fila usa para mostrar a qualidade de cada faixa.
#[uniffi::export]
pub fn probe_track(path: String) -> Result<TrackProbe, PlayerError> {
    let source = AudioSource::open(&path).map_err(|message| PlayerError::Load { message })?;
    let format = source.format();
    Ok(TrackProbe {
        sample_rate: format.sample_rate,
        bit_depth: format.bit_depth,
        channels: format.channels,
        codec: source.codec_name().to_string(),
        total_seconds: source.total_frames() as f64 / format.sample_rate,
    })
}

/// Interpreta `35`, `35%` ou `-18dB` do mesmo jeito que a linha de comando, para a interface
/// não precisar reimplementar a regra.
#[uniffi::export]
pub fn parse_volume_text(text: String) -> Option<f32> {
    let parsed: VolumeRequest = parse_volume(&text);
    if !parsed.valid || parsed.unit != VolumeUnit::Percent {
        return None;
    }
    // `value` já vem como escalar de 0 a 1; dividir por 100 aqui daria 0,0035 para "35".
    Some(parsed.value as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{get_property, property_address};
    use coreaudio_sys::{kAudioDevicePropertyNominalSampleRate, kAudioObjectPropertyScopeGlobal};

    #[test]
    fn shutdown_fecha_a_fronteira_para_comandos_posteriores() {
        let player = HogPlayer::new();
        player
            .shutdown()
            .expect("shutdown em idle deveria funcionar");

        assert_eq!(player.append_tracks(vec!["a.flac".to_string()]), 0);
        assert!(matches!(
            player.select_track(0),
            Err(PlayerError::State { .. })
        ));
        assert_eq!(player.snapshot().state, PlayerState::Idle);
        assert_eq!(player.snapshot().queue_len, 0);
    }

    #[test]
    fn sondar_devolve_o_formato_que_o_engine_vai_ver() {
        let probe = probe_track(crate::fixtures::path("t96_24.flac")).expect("deveria sondar");
        assert_eq!(probe.sample_rate, 96_000.0);
        assert_eq!(probe.bit_depth, 24);
        assert_eq!(probe.channels, 2);
        assert_eq!(probe.codec, "flac");
        assert!(
            (probe.total_seconds - 6.0).abs() < 0.01,
            "{}",
            probe.total_seconds
        );
    }

    #[test]
    fn sondar_arquivo_que_nao_abre_e_falha_de_carga() {
        let result = probe_track("/tmp/hog-audio-nao-existe-sonda.flac".to_string());
        assert!(matches!(result, Err(PlayerError::Load { .. })));
    }

    #[test]
    fn cada_comando_de_transporte_mede_do_zero() {
        let player = HogPlayer::new();
        player.append_tracks(vec!["/tmp/hog-audio-nao-existe-medicao.flac".to_string()]);

        assert!(player.select_track(0).is_err());
        let carga = player.last_timings();
        assert!(carga.open_ms > 0.0, "a abertura que falhou também custa");
        assert_eq!(carga.acquire_ms, 0.0);

        // Nada carregado: o play é recusado pela transição, antes de abrir arquivo ou tocar
        // no device. Se a medição acumulasse entre comandos, a abertura anterior apareceria.
        assert!(matches!(player.play(), Err(PlayerError::State { .. })));
        assert_eq!(player.last_timings().open_ms, 0.0);
    }

    #[test]
    #[ignore = "precisa de um device de saída real; toma o device por alguns segundos"]
    fn tocar_mede_todas_as_etapas_da_partida() {
        let player = HogPlayer::new();
        player.append_tracks(vec![crate::fixtures::path("t96_24.flac")]);
        player.play().expect("deveria tocar");

        let timings = player.last_timings();
        assert!(timings.reopen_ms > 0.0);
        assert!(timings.acquire_ms > 0.0);
        assert!(timings.prefill_ms > 0.0);
        assert!(timings.start_ms > 0.0);
        player.shutdown().expect("deveria restaurar o device");
    }

    #[test]
    #[ignore = "precisa de um device de saída real; toma o device por alguns segundos"]
    fn trocar_faixa_muda_o_rate_fisico_para_o_da_nova_faixa() {
        let player = HogPlayer::new();
        player.append_tracks(vec![
            crate::fixtures::path("t96_24.flac"),
            crate::fixtures::path("t44_16.flac"),
        ]);
        player.play().expect("deveria tocar a primeira faixa");

        let loaded = player
            .next_track()
            .expect("deveria carregar e tocar a segunda");
        assert_eq!(loaded.sample_rate, 44_100.0);
        assert_eq!(player.engine.state(), PlayerState::Playing);

        let device = player
            .engine
            .current_device_id_for_test()
            .expect("deveria manter o device adquirido");
        let address = property_address(
            kAudioDevicePropertyNominalSampleRate,
            kAudioObjectPropertyScopeGlobal,
        );
        let rate: f64 = get_property(device, &address).expect("deveria ler o rate do device");
        assert!((rate - 44_100.0).abs() < 0.5, "rate físico ficou em {rate}");
        player.shutdown().expect("deveria restaurar o device");
    }

    #[test]
    #[ignore = "precisa de um device de saída real; toma o device por alguns segundos"]
    fn auto_avanco_pula_faixa_invalida_e_para_na_ultima() {
        let player = HogPlayer::new();
        player.append_tracks(vec![
            crate::fixtures::path("t96_24.flac"),
            "/tmp/hog-audio-faixa-invalida.flac".to_string(),
            crate::fixtures::path("t44_16.flac"),
        ]);
        player.play().expect("deveria tocar a primeira faixa");
        player.engine.force_finished_for_test();

        let loaded = player
            .advance()
            .expect("avanço não deveria falhar")
            .expect("deveria encontrar a terceira faixa");
        assert_eq!(loaded.sample_rate, 44_100.0);
        assert_eq!(player.snapshot().current_index, Some(2));
        assert!(player.queue_items()[1].failure.is_some());

        player.engine.force_finished_for_test();
        assert!(player.advance().expect("fim da fila não é erro").is_none());
        let snapshot = player.snapshot();
        assert_eq!(snapshot.state, PlayerState::Finished);
        assert_eq!(snapshot.current_index, Some(2));
        player.shutdown().expect("deveria restaurar o device");
    }

    #[test]
    #[ignore = "precisa negociar o formato com um device de saída real"]
    fn arquivo_removido_entre_carga_e_play_e_falha_da_faixa() {
        let source = crate::fixtures::path("t44_16.flac");
        let path = std::env::temp_dir().join(format!(
            "hog-audio-playlist-disappeared-{}.flac",
            std::process::id()
        ));
        std::fs::copy(&source, &path).expect("deveria copiar a fixture");

        let player = HogPlayer::new();
        player.append_tracks(vec![path.to_string_lossy().into_owned()]);
        std::fs::remove_file(&path).expect("deveria remover a cópia antes do play");

        assert!(matches!(player.play(), Err(PlayerError::Load { .. })));
        assert!(player.queue_items()[0].failure.is_some());
        assert_eq!(player.snapshot().queue_version, 2);
    }
}
