//! O que uma linha da fila mostra: porte de `QueueEntry.swift`. Até a carga terminar, a linha
//! usa o nome do arquivo.

use std::sync::Arc;

use crate::format::{compact_quality, format_time, quality_detail};
use crate::media::Thumbnail;
use crate::media::artwork::{THUMBNAIL_PIXELS, thumbnail};
use crate::media::tags::{Tags, read_tags};

#[derive(Clone, Debug, PartialEq)]
pub struct QueueEntryInfo {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub quality: Option<String>,
    pub quality_detail: Option<String>,
    pub duration: Option<String>,
    pub thumbnail: Option<Thumbnail>,
    pub problem: Option<String>,
}

impl QueueEntryInfo {
    pub fn subtitle(&self) -> String {
        [self.artist.as_deref(), self.album.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" — ")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProbeOutcome {
    Probed {
        sample_rate: f64,
        bit_depth: Option<u32>,
        bitrate_kbps: Option<u32>,
        codec: String,
        total_seconds: f64,
    },
    Failed(String),
}

pub fn queue_entry_info(
    tags: &Tags,
    probe: ProbeOutcome,
    thumbnail: Option<Thumbnail>,
) -> QueueEntryInfo {
    let (quality, detail, duration, problem) = match probe {
        ProbeOutcome::Probed {
            sample_rate,
            bit_depth,
            bitrate_kbps,
            codec,
            total_seconds,
        } => (
            Some(compact_quality(
                sample_rate,
                bit_depth,
                bitrate_kbps,
                &codec,
            )),
            Some(quality_detail(sample_rate, bit_depth, bitrate_kbps, &codec)),
            Some(format_time(total_seconds)),
            None,
        ),
        ProbeOutcome::Failed(reason) => (None, None, None, Some(reason)),
    };
    QueueEntryInfo {
        title: tags.title.clone(),
        artist: tags.artist.clone(),
        album: tags.album.clone(),
        quality,
        quality_detail: detail,
        duration,
        thumbnail,
        problem,
    }
}

/// Tags pelo lofty e formato pelo mesmo `AudioSource` que o engine usa: o selo mostra o que
/// vai chegar ao DAC, não o que a extensão sugere.
pub fn load_queue_entry(path: &str) -> QueueEntryInfo {
    let tags = read_tags(path);
    let probe = match hog_audio::api::probe_track(path.to_string()) {
        Ok(probe) => ProbeOutcome::Probed {
            sample_rate: probe.sample_rate,
            bit_depth: probe.bit_depth,
            bitrate_kbps: probe.bitrate_kbps,
            codec: probe.codec,
            total_seconds: probe.total_seconds,
        },
        Err(error) => ProbeOutcome::Failed(error.to_string()),
    };
    let thumbnail = tags
        .artwork
        .as_deref()
        .and_then(|bytes| thumbnail(bytes, THUMBNAIL_PIXELS))
        .map(|raster| Thumbnail(Arc::new(raster)));
    queue_entry_info(&tags, probe, thumbnail)
}
