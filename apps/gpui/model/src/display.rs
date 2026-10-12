//! O que a tela mostra, traduzido do `Snapshot`: porte de `DisplayState.swift`. Funções puras,
//! testáveis sem hardware. O relógio fica à parte porque muda a cada tique.

use hog_audio::api::Snapshot;
use hog_audio::transitions::PlayerState;

use crate::format::{format_time, quality_detail};
use crate::media::ArtworkRef;
use crate::metadata::TrackMetadata;
use crate::port::Format;

pub const NO_TRACK_TITLE: &str = "nenhuma faixa carregada";

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayState {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Option<ArtworkRef>,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub is_playing: bool,
    pub technical_line: String,
}

pub fn display_state(
    snapshot: &Snapshot,
    metadata: Option<&TrackMetadata>,
    format: Option<&Format>,
) -> DisplayState {
    const REWIND_THRESHOLD_SECONDS: f64 = 3.0;
    let playing = snapshot.state == PlayerState::Playing;
    // Em Loaded o hog ainda não foi tomado; em Finished o device continua retido, e é quando
    // "por que o Mac segue mudo" mais surpreende.
    let holds_device = matches!(
        snapshot.state,
        PlayerState::Playing | PlayerState::Paused | PlayerState::Finished
    );
    let technical_line = format
        .map(|format| {
            let mut parts = vec![quality_detail(
                format.sample_rate,
                format.bit_depth,
                format.bitrate_kbps,
                &format.codec,
            )];
            if holds_device {
                parts.push("hog ativo".to_string());
            }
            if snapshot.underruns > 0 {
                parts.push(format!("{} underruns", snapshot.underruns));
            }
            parts.join(" · ")
        })
        .unwrap_or_default();

    DisplayState {
        title: metadata.map_or_else(|| NO_TRACK_TITLE.to_string(), |m| m.title.clone()),
        artist: metadata.and_then(|m| m.artist.clone()).unwrap_or_default(),
        album: metadata.and_then(|m| m.album.clone()).unwrap_or_default(),
        artwork: metadata.and_then(|m| m.artwork.clone()),
        can_play: matches!(
            snapshot.state,
            PlayerState::Loaded | PlayerState::Paused | PlayerState::Finished
        ),
        can_pause: playing,
        can_go_next: snapshot
            .current_index
            .is_some_and(|i| i + 1 < snapshot.queue_len),
        can_go_previous: snapshot
            .current_index
            .is_some_and(|i| i > 0 || snapshot.elapsed_seconds >= REWIND_THRESHOLD_SECONDS),
        is_playing: playing,
        technical_line,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClockState {
    pub elapsed: String,
    pub total: String,
    pub progress: f64,
}

pub fn clock_state(snapshot: &Snapshot) -> ClockState {
    let progress = if snapshot.total_seconds > 0.0 {
        (snapshot.elapsed_seconds / snapshot.total_seconds).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ClockState {
        elapsed: format_time(snapshot.elapsed_seconds),
        total: format_time(snapshot.total_seconds),
        progress,
    }
}
