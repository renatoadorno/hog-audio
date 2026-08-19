//! A superfície que o Swift enxerga. Só controle e leitura de estado atravessam esta
//! fronteira: nenhum dado de áudio passa por aqui, e o IOProc nunca a alcança.

use std::sync::Arc;

use crate::engine::Engine;
use crate::transitions::PlayerState;
use crate::volume::{parse_volume, VolumeRequest, VolumeUnit};

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
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            engine: Engine::new(),
        })
    }

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

    pub fn play(&self) -> Result<(), PlayerError> {
        self.engine
            .play()
            .map_err(|message| PlayerError::Device { message })
    }

    pub fn pause(&self) -> Result<(), PlayerError> {
        self.engine
            .pause()
            .map_err(|message| PlayerError::State { message })
    }

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

    pub fn shutdown(&self) {
        let _ = self.engine.shutdown();
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
