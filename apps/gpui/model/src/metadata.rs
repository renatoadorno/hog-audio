//! O que a tela mostra da faixa atual. A leitura vem na Task 3.

use crate::media::ArtworkRef;

#[derive(Clone, Debug, PartialEq)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork: Option<ArtworkRef>,
}
