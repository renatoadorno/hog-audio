//! A fronteira com o core. A trait existe para o controlador ser testado com um player falso;
//! a implementação para o `HogPlayer` só repassa, sem regra nenhuma.

use hog_audio::api::{HogPlayer, PlayerError, QueueItem, Snapshot, TrackFormat, TransitionTimings};

pub trait Player: Send + Sync + 'static {
    fn append_tracks(&self, paths: Vec<String>) -> u32;
    fn queue_items(&self) -> Vec<QueueItem>;
    fn try_queue_items(&self) -> Option<Vec<QueueItem>>;
    fn navigate(&self, steps: i32) -> Result<TrackFormat, PlayerError>;
    fn select_track(&self, index: u32) -> Result<TrackFormat, PlayerError>;
    fn select_track_id(&self, id: u64) -> Result<TrackFormat, PlayerError>;
    fn advance(&self) -> Result<Option<TrackFormat>, PlayerError>;
    fn remove_track_id(&self, id: u64) -> Result<Option<TrackFormat>, PlayerError>;
    fn clear_queue(&self) -> Result<(), PlayerError>;
    fn play(&self) -> Result<(), PlayerError>;
    fn pause(&self) -> Result<(), PlayerError>;
    fn set_volume(&self, scalar: f32) -> Result<(), PlayerError>;
    fn snapshot(&self) -> Snapshot;
    fn recover(&self) -> Result<bool, PlayerError>;
    fn last_problem(&self) -> Option<String>;
    fn last_timings(&self) -> TransitionTimings;
    fn shutdown(&self) -> Result<(), PlayerError>;
}

impl Player for HogPlayer {
    fn append_tracks(&self, paths: Vec<String>) -> u32 {
        HogPlayer::append_tracks(self, paths)
    }
    fn queue_items(&self) -> Vec<QueueItem> {
        HogPlayer::queue_items(self)
    }
    fn try_queue_items(&self) -> Option<Vec<QueueItem>> {
        HogPlayer::try_queue_items(self)
    }
    fn navigate(&self, steps: i32) -> Result<TrackFormat, PlayerError> {
        HogPlayer::navigate(self, steps)
    }
    fn select_track(&self, index: u32) -> Result<TrackFormat, PlayerError> {
        HogPlayer::select_track(self, index)
    }
    fn select_track_id(&self, id: u64) -> Result<TrackFormat, PlayerError> {
        HogPlayer::select_track_id(self, id)
    }
    fn advance(&self) -> Result<Option<TrackFormat>, PlayerError> {
        HogPlayer::advance(self)
    }
    fn remove_track_id(&self, id: u64) -> Result<Option<TrackFormat>, PlayerError> {
        HogPlayer::remove_track_id(self, id)
    }
    fn clear_queue(&self) -> Result<(), PlayerError> {
        HogPlayer::clear_queue(self)
    }
    fn play(&self) -> Result<(), PlayerError> {
        HogPlayer::play(self)
    }
    fn pause(&self) -> Result<(), PlayerError> {
        HogPlayer::pause(self)
    }
    fn set_volume(&self, scalar: f32) -> Result<(), PlayerError> {
        HogPlayer::set_volume(self, scalar)
    }
    fn snapshot(&self) -> Snapshot {
        HogPlayer::snapshot(self)
    }
    fn recover(&self) -> Result<bool, PlayerError> {
        HogPlayer::recover(self)
    }
    fn last_problem(&self) -> Option<String> {
        HogPlayer::last_problem(self)
    }
    fn last_timings(&self) -> TransitionTimings {
        HogPlayer::last_timings(self)
    }
    fn shutdown(&self) -> Result<(), PlayerError> {
        HogPlayer::shutdown(self)
    }
}

/// Uma linha da fila. Os tipos do core não derivam `Clone` nem `PartialEq` (são registros do
/// uniffi), e o modelo precisa dos dois para comparar e para entregar à UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub id: u64,
    pub path: String,
    pub failure: Option<String>,
}

impl From<QueueItem> for Track {
    fn from(item: QueueItem) -> Self {
        Self {
            id: item.id,
            path: item.path,
            failure: item.failure,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Format {
    pub sample_rate: f64,
    pub bit_depth: u32,
    pub codec: String,
}

impl From<&TrackFormat> for Format {
    fn from(format: &TrackFormat) -> Self {
        Self {
            sample_rate: format.sample_rate,
            bit_depth: format.bit_depth,
            codec: format.codec.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Timings {
    pub open_ms: f64,
    pub release_ms: f64,
    pub reopen_ms: f64,
    pub acquire_ms: f64,
    pub volume_ms: f64,
    pub prefill_ms: f64,
    pub start_ms: f64,
}

impl From<TransitionTimings> for Timings {
    fn from(t: TransitionTimings) -> Self {
        Self {
            open_ms: t.open_ms,
            release_ms: t.release_ms,
            reopen_ms: t.reopen_ms,
            acquire_ms: t.acquire_ms,
            volume_ms: t.volume_ms,
            prefill_ms: t.prefill_ms,
            start_ms: t.start_ms,
        }
    }
}
