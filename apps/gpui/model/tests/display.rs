mod support;

use hog_audio::transitions::PlayerState;
use hog_player_model::display::{NO_TRACK_TITLE, clock_state, display_state};
use hog_player_model::metadata::TrackMetadata;
use hog_player_model::port::Format;
use support::snapshot;

fn faixa() -> Format {
    Format {
        sample_rate: 96_000.0,
        bit_depth: 24,
        codec: "flac".to_string(),
    }
}

fn tags() -> TrackMetadata {
    TrackMetadata {
        title: "House of Memories".to_string(),
        artist: Some("Panic! At The Disco".to_string()),
        album: Some("Death of a Bachelor".to_string()),
        artwork: None,
    }
}

#[test]
fn tocando_mostra_pausar() {
    let d = display_state(
        &snapshot(PlayerState::Playing, 83.0, 0, Some(0), 1),
        Some(&tags()),
        Some(&faixa()),
    );
    assert!(d.is_playing);
    assert!(d.can_pause);
    assert_eq!(d.title, "House of Memories");
}

#[test]
fn o_relogio_mostra_o_tempo_e_o_progresso() {
    let c = clock_state(&snapshot(PlayerState::Playing, 83.0, 0, Some(0), 1));
    assert_eq!(c.elapsed, "1:23");
    assert_eq!(c.total, "3:28");
    assert!((c.progress - 83.0 / 208.7).abs() < 0.001);
}

#[test]
fn sem_faixa_carregada_nao_da_para_tocar() {
    let mut vazio = snapshot(PlayerState::Idle, 0.0, 0, None, 0);
    vazio.total_seconds = 0.0;
    let d = display_state(&vazio, None, None);
    assert!(!d.can_play);
    assert!(!d.can_pause);
    assert_eq!(d.title, NO_TRACK_TITLE);
    assert_eq!(clock_state(&vazio).progress, 0.0);
    assert!(d.technical_line.is_empty());
}

#[test]
fn a_linha_tecnica_mostra_o_que_o_projeto_faz_de_diferente() {
    let d = display_state(
        &snapshot(PlayerState::Playing, 0.0, 0, Some(0), 1),
        Some(&tags()),
        Some(&faixa()),
    );
    assert!(d.technical_line.contains("96 kHz"));
    assert!(d.technical_line.contains("24 bit"));
    assert!(d.technical_line.contains("flac"));
    assert!(d.technical_line.contains("hog ativo"));
}

#[test]
fn pausado_ainda_segura_o_device() {
    let d = display_state(
        &snapshot(PlayerState::Paused, 40.0, 0, Some(0), 1),
        Some(&tags()),
        Some(&faixa()),
    );
    assert!(!d.is_playing);
    assert!(d.can_play);
    assert!(d.technical_line.contains("hog ativo"));
}

#[test]
fn carregado_ainda_nao_segura_o_device() {
    let d = display_state(
        &snapshot(PlayerState::Loaded, 0.0, 0, Some(0), 1),
        Some(&tags()),
        Some(&faixa()),
    );
    assert!(d.can_play);
    assert!(!d.technical_line.contains("hog ativo"));
}

#[test]
fn underruns_aparecem_na_linha_tecnica() {
    let d = display_state(
        &snapshot(PlayerState::Playing, 0.0, 3, Some(0), 1),
        Some(&tags()),
        Some(&faixa()),
    );
    assert!(d.technical_line.contains("3 underruns"));
}

#[test]
fn taxa_derivada_de_cd_mostra_a_casa_decimal() {
    let cd = Format {
        sample_rate: 44_100.0,
        bit_depth: 16,
        codec: "flac".to_string(),
    };
    let d = display_state(
        &snapshot(PlayerState::Playing, 0.0, 0, Some(0), 1),
        Some(&tags()),
        Some(&cd),
    );
    assert!(d.technical_line.contains("44.1 kHz"));
}

#[test]
fn a_faixa_terminada_pode_ser_tocada_de_novo() {
    let d = display_state(
        &snapshot(PlayerState::Finished, 208.7, 0, Some(0), 1),
        Some(&tags()),
        Some(&faixa()),
    );
    assert!(d.can_play);
    assert!(!d.can_pause);
    assert!(d.technical_line.contains("hog ativo"));
}

#[test]
fn navegacao_respeita_pontas_e_regra_dos_tres_segundos() {
    let first = display_state(
        &snapshot(PlayerState::Playing, 2.9, 0, Some(0), 3),
        Some(&tags()),
        Some(&faixa()),
    );
    assert!(!first.can_go_previous);
    assert!(first.can_go_next);

    let rewind = display_state(
        &snapshot(PlayerState::Playing, 3.0, 0, Some(0), 3),
        Some(&tags()),
        Some(&faixa()),
    );
    assert!(rewind.can_go_previous);

    let last = display_state(
        &snapshot(PlayerState::Paused, 0.0, 0, Some(2), 3),
        Some(&tags()),
        Some(&faixa()),
    );
    assert!(last.can_go_previous);
    assert!(!last.can_go_next);
}
