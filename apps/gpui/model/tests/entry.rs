mod support;

use hog_player_model::entry::{ProbeOutcome, load_queue_entry, queue_entry_info};
use hog_player_model::media::tags::Tags;
use hog_player_model::metadata::load_track_metadata;
use support::fixture;

fn tags(artist: Option<&str>, album: Option<&str>) -> Tags {
    Tags {
        title: "Life in Technicolor".to_string(),
        artist: artist.map(str::to_string),
        album: album.map(str::to_string),
        artwork: None,
    }
}

fn sonda() -> ProbeOutcome {
    ProbeOutcome::Probed {
        sample_rate: 44_100.0,
        bit_depth: Some(24),
        bitrate_kbps: None,
        codec: "flac".to_string(),
        total_seconds: 149.6,
    }
}

#[test]
fn subtitulo_junta_artista_e_album_so_quando_existem() {
    let ambos = queue_entry_info(&tags(Some("Coldplay"), Some("Viva La Vida")), sonda(), None);
    assert_eq!(ambos.subtitle(), "Coldplay — Viva La Vida");
    assert_eq!(
        queue_entry_info(&tags(Some("Coldplay"), None), sonda(), None).subtitle(),
        "Coldplay"
    );
    assert_eq!(
        queue_entry_info(&tags(None, Some("Viva La Vida")), sonda(), None).subtitle(),
        "Viva La Vida"
    );
    assert!(
        queue_entry_info(&tags(None, None), sonda(), None)
            .subtitle()
            .is_empty()
    );
}

#[test]
fn sondagem_bem_sucedida_preenche_selo_tooltip_e_duracao() {
    let info = queue_entry_info(&tags(Some("Coldplay"), Some("Viva La Vida")), sonda(), None);
    assert_eq!(info.title, "Life in Technicolor");
    assert_eq!(info.quality.as_deref(), Some("24/44.1"));
    assert_eq!(
        info.quality_detail.as_deref(),
        Some("44.1 kHz · 24 bit · flac")
    );
    assert_eq!(info.duration.as_deref(), Some("2:29"));
    assert!(info.problem.is_none());
}

#[test]
fn sondagem_que_falha_vira_problema_sem_qualidade() {
    let info = queue_entry_info(
        &tags(Some("Coldplay"), None),
        ProbeOutcome::Failed("não consegui abrir o arquivo".to_string()),
        None,
    );
    assert_eq!(
        info.problem.as_deref(),
        Some("não consegui abrir o arquivo")
    );
    assert!(info.quality.is_none());
    assert!(info.duration.is_none());
    assert_eq!(info.title, "Life in Technicolor");
}

#[test]
fn carga_real_usa_a_mesma_sondagem_do_engine() {
    let info = load_queue_entry(&fixture("t96_24.flac"));
    assert_eq!(info.quality.as_deref(), Some("24/96"));
    assert_eq!(
        info.quality_detail.as_deref(),
        Some("96 kHz · 24 bit · flac")
    );
    assert_eq!(info.duration.as_deref(), Some("0:06"));
    assert!(info.problem.is_none());
}

#[test]
fn carga_real_de_aac_mostra_codec_e_bitrate() {
    let path = fixture("t44_aac.m4a");
    // O bitrate do AAC é VBR e depende do encoder: o esperado vem da mesma sondagem do core.
    let kbps = hog_audio::api::probe_track(path.clone())
        .expect("deveria sondar o AAC")
        .bitrate_kbps
        .expect("o AAC deveria ter bitrate");
    let info = load_queue_entry(&path);
    assert_eq!(info.problem, None);
    assert_eq!(info.quality, Some(format!("AAC {kbps}")));
    assert_eq!(
        info.quality_detail,
        Some(format!("44.1 kHz · AAC {kbps} kbps"))
    );
}

#[test]
fn carga_real_com_capa_traz_tags_e_miniatura() {
    let info = load_queue_entry(&fixture("meta_fixture.flac"));
    assert_eq!(info.title, "Titulo Teste");
    assert_eq!(info.subtitle(), "Artista Teste — Album Teste");
    assert!(info.thumbnail.is_some());
}

#[test]
fn arquivo_que_nao_abre_vira_problema_com_a_mensagem_do_core() {
    let info = load_queue_entry("/tmp/hog-audio-nao-existe-fila.flac");
    let problem = info.problem.expect("devia virar problema");
    assert!(!problem.is_empty());
    assert!(
        !problem.starts_with("Load"),
        "a mensagem do core, não o Debug do enum: {problem}"
    );
    assert_eq!(info.title, "hog-audio-nao-existe-fila");
}

#[test]
fn metadados_da_faixa_atual_trazem_capa_e_fundo() {
    let meta = load_track_metadata(&fixture("meta_fixture.flac"));
    assert_eq!(meta.title, "Titulo Teste");
    let art = meta.artwork.expect("capa");
    assert!(art.0.cover.width > 0);
    assert!(art.0.backdrop.is_some());
}

#[test]
fn faixa_sem_capa_nao_tem_artwork() {
    assert!(
        load_track_metadata(&fixture("meta_fixture_sem_capa.flac"))
            .artwork
            .is_none()
    );
}
