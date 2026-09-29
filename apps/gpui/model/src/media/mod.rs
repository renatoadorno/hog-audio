//! Imagens já decodificadas, prontas para a UI enviar à GPU sem trabalho na main thread.

pub mod artwork;
pub mod tags;

use std::sync::Arc;

/// Pixels em BGRA, a ordem que o `RenderImage` do GPUI envia (conferida na Task 0).
#[derive(Debug, PartialEq, Eq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// A capa da faixa atual e o fundo já desfocado, gerados uma vez por faixa fora da main.
#[derive(Debug)]
pub struct Artwork {
    pub cover: Raster,
    pub backdrop: Option<Raster>,
}

/// Comparados por ponteiro: o tique compara o modelo dez vezes por segundo, e comparar pixels
/// seria caro. Uma capa nova sempre chega num `Arc` novo.
#[derive(Clone, Debug)]
pub struct ArtworkRef(pub Arc<Artwork>);

impl PartialEq for ArtworkRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Debug)]
pub struct Thumbnail(pub Arc<Raster>);

impl PartialEq for Thumbnail {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
