//! `Raster` (BGRA, do modelo) para `RenderImage` (GPU). A conversão só copia bytes: decodificar
//! e reduzir já aconteceu fora da main. As imagens que saem de uso saem do atlas da GPU.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{App, RenderImage};
use hog_player_model::media::{Artwork, ArtworkRef, Raster, Thumbnail};

pub fn render_image(raster: &Raster) -> Option<Arc<RenderImage>> {
    let buffer = image::RgbaImage::from_raw(raster.width, raster.height, raster.bgra.clone())?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// Tira a imagem do atlas depois do quadro. A fila converte e solta miniaturas desenhando, e
/// durante o desenho a janela sai de `App.windows`: o `drop_image(.., None)` chamado ali não a
/// alcançaria, e a imagem ficaria no atlas.
fn release(image: Arc<RenderImage>, cx: &mut App) {
    cx.defer(move |cx| cx.drop_image(image, None));
}

/// Uma miniatura por caminho da fila, como as linhas do modelo.
#[derive(Default)]
pub struct ThumbnailCache {
    images: HashMap<String, (Thumbnail, Arc<RenderImage>)>,
}

impl ThumbnailCache {
    pub fn get(
        &mut self,
        path: &str,
        thumbnail: &Thumbnail,
        cx: &mut App,
    ) -> Option<Arc<RenderImage>> {
        if let Some((cached, image)) = self.images.get(path)
            && cached == thumbnail
        {
            return Some(Arc::clone(image));
        }
        let image = render_image(&thumbnail.0)?;
        if let Some((_, old)) = self
            .images
            .insert(path.to_string(), (thumbnail.clone(), Arc::clone(&image)))
        {
            release(old, cx);
        }
        Some(image)
    }

    pub fn retain(&mut self, keep: impl Fn(&str) -> bool, cx: &mut App) {
        let gone: Vec<String> = self
            .images
            .keys()
            .filter(|path| !keep(path))
            .cloned()
            .collect();
        for path in gone {
            if let Some((_, image)) = self.images.remove(&path) {
                release(image, cx);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

/// A imagem de uma capa (a grande ou o fundo) da faixa atual, convertida uma vez por capa.
#[derive(Default)]
pub struct ArtworkSlot {
    shown: Option<(ArtworkRef, Arc<RenderImage>)>,
}

impl ArtworkSlot {
    /// Devolve a imagem a mostrar e a que saiu de uso, para a view tirar do atlas.
    pub fn update(
        &mut self,
        next: Option<&ArtworkRef>,
        pick: impl Fn(&Artwork) -> Option<&Raster>,
    ) -> (Option<Arc<RenderImage>>, Option<Arc<RenderImage>>) {
        if let (Some(next), Some((shown, image))) = (next, &self.shown)
            && next == shown
        {
            return (Some(Arc::clone(image)), None);
        }
        let old = self.shown.take().map(|(_, image)| image);
        self.shown = next.and_then(|art| {
            pick(&art.0)
                .and_then(render_image)
                .map(|image| (art.clone(), image))
        });
        (self.shown.as_ref().map(|(_, image)| Arc::clone(image)), old)
    }
}
