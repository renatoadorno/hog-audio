use std::sync::Arc;

use gpui::TestAppContext;
use hog_player_gpui::images::{ArtworkSlot, ThumbnailCache, render_image};
use hog_player_model::media::{Artwork, ArtworkRef, Raster, Thumbnail};

fn raster(width: u32, height: u32) -> Raster {
    Raster {
        width,
        height,
        bgra: [0, 0, 255, 255].repeat((width * height) as usize),
    }
}

fn arte() -> ArtworkRef {
    ArtworkRef(Arc::new(Artwork {
        cover: raster(4, 4),
        backdrop: Some(raster(2, 2)),
    }))
}

#[test]
fn raster_invalido_nao_vira_imagem() {
    let quebrado = Raster {
        width: 4,
        height: 4,
        bgra: vec![0; 3],
    };
    assert!(render_image(&quebrado).is_none());
    assert!(render_image(&raster(2, 1)).is_some());
}

#[test]
fn a_mesma_capa_mostra_a_mesma_imagem_e_nada_sai() {
    let mut slot = ArtworkSlot::default();
    let capa = arte();
    let (primeira, saiu) = slot.update(Some(&capa), |a| Some(&a.cover));
    assert!(primeira.is_some() && saiu.is_none());

    let (segunda, saiu) = slot.update(Some(&capa), |a| Some(&a.cover));
    assert!(Arc::ptr_eq(
        primeira.as_ref().unwrap(),
        segunda.as_ref().unwrap()
    ));
    assert!(saiu.is_none());
}

#[test]
fn capa_nova_troca_a_imagem_e_a_velha_sai() {
    let mut slot = ArtworkSlot::default();
    let (velha, _) = slot.update(Some(&arte()), |a| Some(&a.cover));
    let (nova, saiu) = slot.update(Some(&arte()), |a| Some(&a.cover));

    assert!(!Arc::ptr_eq(
        velha.as_ref().unwrap(),
        nova.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(velha.as_ref().unwrap(), saiu.as_ref().unwrap()));

    let (nenhuma, saiu) = slot.update(None, |a| Some(&a.cover));
    assert!(nenhuma.is_none());
    assert!(Arc::ptr_eq(nova.as_ref().unwrap(), saiu.as_ref().unwrap()));
}

#[gpui::test]
fn a_miniatura_e_convertida_uma_vez_e_sai_quando_a_faixa_sai(cx: &mut TestAppContext) {
    let miniatura = Thumbnail(Arc::new(raster(2, 2)));
    cx.update(|cx| {
        let mut cache = ThumbnailCache::default();
        let a = cache.get("/tmp/a.flac", &miniatura, cx).expect("imagem");
        let b = cache.get("/tmp/a.flac", &miniatura, cx).expect("imagem");
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(cache.len(), 1);

        cache.retain(|path| path != "/tmp/a.flac", cx);
        assert_eq!(cache.len(), 0);
    });
}
