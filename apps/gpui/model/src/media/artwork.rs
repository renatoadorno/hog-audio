//! Miniatura, capa e fundo desfocado a partir dos bytes da capa embutida. Tudo roda fora da
//! main thread e sai em BGRA, pronto para o GPUI enviar à GPU. A técnica (desfocar na CPU uma
//! versão pequena) vem do Pawse; os números vêm do SwiftUI, para a comparação medir a
//! tecnologia e não um visual diferente.

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};

use super::{Artwork, Raster};

/// 44 pt em Retina são 88 px. A fila guarda só isto: a capa inteira tem de 1 a 5 MB.
pub const THUMBNAIL_PIXELS: u32 = 96;
/// A coluna tem no máximo 560 pt, 1120 px em Retina.
pub const COVER_PIXELS: u32 = 1200;
/// O fundo não precisa de detalhe: reduzir antes de desfocar deixa o blur barato.
pub const BACKDROP_PIXELS: u32 = 160;

pub fn thumbnail(bytes: &[u8], max_pixels: u32) -> Option<Raster> {
    let image = decode(bytes)?;
    Some(to_bgra(&fit(image, max_pixels)))
}

pub fn backdrop(bytes: &[u8]) -> Option<Raster> {
    let image = decode(bytes)?;
    Some(blurred(&image))
}

/// Decodifica uma vez só e gera a capa e o fundo da faixa atual.
pub fn artwork(bytes: &[u8]) -> Option<Artwork> {
    let image = decode(bytes)?;
    let backdrop = blurred(&image);
    Some(Artwork {
        cover: to_bgra(&fit(image, COVER_PIXELS)),
        backdrop: Some(backdrop),
    })
}

/// Um cabeçalho PNM pode declarar lado 0 e decodificar sem erro; imagem sem pixels não é capa.
fn decode(bytes: &[u8]) -> Option<DynamicImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    Some(image)
}

/// Reduz para caber em `max_pixels` no lado maior. Imagem menor fica como está.
fn fit(image: DynamicImage, max_pixels: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width.max(height) <= max_pixels {
        image
    } else {
        image.resize(max_pixels, max_pixels, FilterType::Triangle)
    }
}

fn blurred(image: &DynamicImage) -> Raster {
    const SIGMA: f32 = 10.0;
    // Recorta antes de reduzir: o `resize_to_fill` amplia o lado menor até `BACKDROP_PIXELS` antes
    // de recortar, e uma capa 1×N viraria um intermediário de `BACKDROP_PIXELS`² × N pixels.
    let (width, height) = image.dimensions();
    let side = width.min(height);
    let square = image.crop_imm((width - side) / 2, (height - side) / 2, side, side);
    let small = square.resize_exact(BACKDROP_PIXELS, BACKDROP_PIXELS, FilterType::Triangle);
    let blurred = image::imageops::fast_blur(&small.to_rgba8(), SIGMA);
    to_bgra(&DynamicImage::ImageRgba8(blurred))
}

fn to_bgra(image: &DynamicImage) -> Raster {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut bgra = rgba.into_raw();
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Raster {
        width,
        height,
        bgra,
    }
}
