mod support;

use std::borrow::Cow;
use std::time::{Duration, Instant};

use hog_player_model::media::artwork::{
    BACKDROP_PIXELS, COVER_PIXELS, THUMBNAIL_PIXELS, artwork, backdrop, thumbnail,
};
use hog_player_model::media::tags::{clean_text, read_tags};
use support::fixture;

fn png(pixels: image::RgbImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(pixels)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("png");
    bytes
}

fn solid(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    png(image::RgbImage::from_pixel(width, height, image::Rgb(rgb)))
}

#[test]
fn tags_do_flac_com_capa() {
    let tags = read_tags(&fixture("meta_fixture.flac"));
    assert_eq!(tags.title, "Titulo Teste");
    assert_eq!(tags.artist.as_deref(), Some("Artista Teste"));
    assert_eq!(tags.album.as_deref(), Some("Album Teste"));
    assert!(
        tags.artwork.is_some(),
        "a capa do FLAC vem de METADATA_BLOCK_PICTURE"
    );
}

#[test]
fn flac_sem_capa_tem_tags_e_nao_tem_capa() {
    let tags = read_tags(&fixture("meta_fixture_sem_capa.flac"));
    assert_eq!(tags.title, "Titulo Teste");
    assert!(tags.artwork.is_none());
}

#[test]
fn m4a_e_mp3_trazem_tags_e_capa() {
    for name in ["meta_fixture.m4a", "meta_fixture.mp3"] {
        let tags = read_tags(&fixture(name));
        assert_eq!(tags.title, "Titulo Teste", "{name}");
        assert!(tags.artwork.is_some(), "{name}");
    }
}

#[test]
fn arquivo_que_nao_abre_vira_o_nome_do_arquivo() {
    let tags = read_tags("/tmp/hog-audio-nao-existe-tags.flac");
    assert_eq!(tags.title, "hog-audio-nao-existe-tags");
    assert!(tags.artist.is_none());
    assert!(tags.album.is_none());
    assert!(tags.artwork.is_none());
}

#[test]
fn texto_vazio_ou_so_espacos_nao_vale() {
    assert_eq!(clean_text(Some(Cow::from("   "))), None);
    assert_eq!(
        clean_text(Some(Cow::from(" Coldplay \n"))),
        Some("Coldplay".to_string())
    );
    assert_eq!(clean_text(None), None);
}

#[test]
fn miniatura_cabe_no_tamanho_pedido_e_mantem_a_proporcao() {
    let raster =
        thumbnail(&solid(3000, 1000, [10, 120, 200]), THUMBNAIL_PIXELS).expect("miniatura");
    assert_eq!((raster.width, raster.height), (96, 32));
    assert_eq!(raster.bgra.len(), 96 * 32 * 4);
}

#[test]
fn miniatura_nao_amplia_imagem_pequena() {
    let raster = thumbnail(&solid(50, 40, [1, 2, 3]), THUMBNAIL_PIXELS).expect("miniatura");
    assert_eq!((raster.width, raster.height), (50, 40));
}

#[test]
fn capa_enorme_vira_no_maximo_1200() {
    let art = artwork(&solid(3000, 3000, [200, 10, 10])).expect("capa");
    assert_eq!(
        (art.cover.width, art.cover.height),
        (COVER_PIXELS, COVER_PIXELS)
    );
    assert!(art.backdrop.is_some());
}

#[test]
fn bytes_que_nao_sao_imagem_nao_viram_capa() {
    let lixo = b"nao e imagem";
    assert!(thumbnail(lixo, THUMBNAIL_PIXELS).is_none());
    assert!(backdrop(lixo).is_none());
    assert!(artwork(lixo).is_none());
}

/// Só é significativo no `--workspace` (o modo do `make gpui-test`): o GPUI liga o formato PNM do
/// `image` por unificação de features. Com `-p hog-player-model` sozinho, o PNM não decodifica.
#[test]
fn imagem_sem_pixels_nao_vira_capa() {
    for bytes in [&b"P6\n0 0\n255\n"[..], &b"P6\n0 5\n255\n"[..]] {
        let header = String::from_utf8_lossy(bytes);
        assert!(
            thumbnail(bytes, THUMBNAIL_PIXELS).is_none(),
            "thumbnail de {header:?}"
        );
        assert!(backdrop(bytes).is_none(), "backdrop de {header:?}");
        assert!(artwork(bytes).is_none(), "artwork de {header:?}");
    }
}

#[test]
fn os_canais_saem_na_ordem_bgra() {
    let raster = thumbnail(&solid(8, 8, [255, 0, 0]), THUMBNAIL_PIXELS).expect("miniatura");
    assert_eq!(&raster.bgra[..4], &[0, 0, 255, 255]);
}

#[test]
fn o_fundo_tem_tamanho_fixo() {
    let raster = backdrop(&solid(640, 120, [10, 120, 200])).expect("fundo");
    assert_eq!(
        (raster.width, raster.height),
        (BACKDROP_PIXELS, BACKDROP_PIXELS)
    );
}

#[test]
fn o_fundo_desfoca_uma_borda_dura() {
    let mut split = image::RgbImage::new(128, 128);
    for (x, _, pixel) in split.enumerate_pixels_mut() {
        *pixel = if x < 64 {
            image::Rgb([0, 0, 0])
        } else {
            image::Rgb([255, 255, 255])
        };
    }
    let raster = backdrop(&png(split)).expect("fundo");
    let row = (BACKDROP_PIXELS / 2) as usize * BACKDROP_PIXELS as usize * 4;
    let seam = raster.bgra[row + (BACKDROP_PIXELS / 2) as usize * 4];
    assert!(
        (60..=195).contains(&seam),
        "a borda {seam} devia ter virado gradiente"
    );
}

#[test]
fn capa_de_proporcao_extrema_nao_trava_o_fundo() {
    const HEIGHT: u32 = 4000;
    const CEILING: Duration = Duration::from_secs(2);
    let bytes = solid(1, HEIGHT, [10, 120, 200]);
    let started = Instant::now();
    let raster = backdrop(&bytes).expect("fundo");
    let elapsed = started.elapsed();
    assert_eq!(
        (raster.width, raster.height),
        (BACKDROP_PIXELS, BACKDROP_PIXELS)
    );
    assert!(
        elapsed < CEILING,
        "o fundo de 1x{HEIGHT} levou {elapsed:?}; o teto é {CEILING:?}"
    );
}

#[test]
fn a_capa_real_da_fixture_vira_miniatura() {
    let bytes = read_tags(&fixture("meta_fixture.flac"))
        .artwork
        .expect("capa");
    let raster = thumbnail(&bytes, THUMBNAIL_PIXELS).expect("miniatura");
    assert!(raster.width.max(raster.height) <= THUMBNAIL_PIXELS);
    assert!(raster.width > 0 && raster.height > 0);
}
