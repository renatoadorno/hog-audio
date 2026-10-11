use hog_player_model::layout::{
    COLUMN_SPACING, CONTROLS_HEIGHT, COVER_MAX, COVER_MIN, MIN_QUEUE_WIDTH, MIN_WINDOW, PADDING,
    cover_side,
};

fn queue_width(width: f32, cover: f32) -> f32 {
    width - 2.0 * PADDING - COLUMN_SPACING - cover
}

#[test]
fn no_tamanho_minimo_capa_e_fila_cabem() {
    let (width, height) = MIN_WINDOW;
    let cover = cover_side(width, height);
    assert!(cover >= COVER_MIN);
    assert!(queue_width(width, cover) >= MIN_QUEUE_WIDTH);
    assert!(cover + CONTROLS_HEIGHT + 2.0 * PADDING <= height);
}

#[test]
fn com_largura_sobrando_a_capa_cresce_com_a_altura() {
    let baixa = cover_side(1400.0, 700.0);
    let alta = cover_side(1400.0, 800.0);
    assert_eq!(alta - baixa, 100.0);
}

#[test]
fn largura_curta_encolhe_a_capa_para_preservar_a_fila() {
    let cover = cover_side(820.0, 1000.0);
    assert_eq!(queue_width(820.0, cover), MIN_QUEUE_WIDTH);
}

#[test]
fn capa_nunca_passa_do_teto() {
    assert_eq!(cover_side(3000.0, 3000.0), COVER_MAX);
}
