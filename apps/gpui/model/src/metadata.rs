//! O que a tela mostra da faixa atual, com a capa grande e o fundo já prontos.

use std::sync::Arc;

use crate::media::ArtworkRef;
use crate::media::artwork::artwork;
use crate::media::tags::read_tags;

#[derive(Clone, Debug, PartialEq)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork: Option<ArtworkRef>,
}

/// Roda fora da main: decodificar uma capa de 3000 px e desfocar leva dezenas de ms.
pub fn load_track_metadata(path: &str) -> TrackMetadata {
    let tags = read_tags(path);
    let artwork = tags
        .artwork
        .as_deref()
        .and_then(artwork)
        .map(|art| ArtworkRef(Arc::new(art)));
    TrackMetadata {
        title: tags.title,
        artist: tags.artist,
        album: tags.album,
        artwork,
    }
}
