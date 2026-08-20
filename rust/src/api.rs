//! A superfície que o Swift enxerga. Só controle e leitura de estado atravessam esta
//! fronteira: nenhum dado de áudio passa por aqui, e o IOProc nunca a alcança.

use std::sync::Arc;

use crate::engine::{Engine, PlayFailure};
use crate::transitions::PlayerState;
use crate::volume::{VolumeRequest, VolumeUnit, parse_volume};

#[derive(uniffi::Record)]
pub struct Snapshot {
    pub state: PlayerState,
    pub elapsed_seconds: f64,
    pub total_seconds: f64,
    pub underruns: u64,
    pub volume_scalar: f32,
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
}

#[uniffi::export]
impl HogPlayer {
    /// Cria um player novo, em repouso (`Idle`) e sem device nenhum tomado — só `load` toca no
    /// hardware.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            engine: Engine::new(),
        })
    }

    /// Decodifica o cabeçalho e negocia o formato com o device padrão, sem tocar nada ainda —
    /// isso fica a cargo de `play`. Um caminho inválido ou uma negociação recusada não mudam o
    /// que já estava carregado antes.
    pub fn load(&self, path: String) -> Result<TrackFormat, PlayerError> {
        let track = self
            .engine
            .load(&path)
            .map_err(|message| PlayerError::Load { message })?;
        Ok(TrackFormat {
            sample_rate: track.sample_rate,
            bit_depth: track.bit_depth,
            channels: track.channels,
            codec: track.codec,
            device_name: track.device_name,
            total_seconds: track.total_seconds,
        })
    }

    /// Começa ou retoma a reprodução. As duas causas de falha chegam com tipos diferentes de
    /// propósito: `PlayerError::State` quando o pedido não faz sentido no estado atual (nada
    /// carregado, ou uma falha anterior que só se resolve recarregando) — nada que reter o
    /// hardware; `PlayerError::Device` quando o device recusou ou não respondeu.
    pub fn play(&self) -> Result<(), PlayerError> {
        self.engine
            .play_classified()
            .map_err(|failure| match failure {
                PlayFailure::State(message) => PlayerError::State { message },
                PlayFailure::Device(message) => PlayerError::Device { message },
            })
    }

    /// Suspende a reprodução sem soltar o device — retomar depois com `play` é rápido porque o
    /// hog mode continua ativo. Só existe uma causa de falha aqui, a transição de estado, então
    /// chega sempre como `PlayerError::State`.
    pub fn pause(&self) -> Result<(), PlayerError> {
        self.engine
            .pause()
            .map_err(|message| PlayerError::State { message })
    }

    /// Aplica o volume direto no device, nunca em software sobre as amostras — é assim que a
    /// reprodução continua bit-perfect. `scalar` é grampeado em 0..1 antes de chegar ao
    /// hardware, então o Swift não precisa validar o próprio input.
    pub fn set_volume(&self, scalar: f32) -> Result<(), PlayerError> {
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
        }
    }

    /// Solta o device e devolve-o ao estado original — sample rate, formato e volume. Propaga
    /// a falha de restauração em vez de engolir: sem isso o app fecharia e o Mac podia ficar
    /// com outro sample rate sem ninguém saber.
    pub fn shutdown(&self) -> Result<(), PlayerError> {
        self.engine
            .shutdown()
            .map_err(|message| PlayerError::Device { message })
    }
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
