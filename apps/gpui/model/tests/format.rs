use hog_player_model::format::{compact_quality, format_time, quality_detail};

#[test]
fn formata_segundos_como_relogio() {
    assert_eq!(format_time(0.0), "0:00");
    assert_eq!(format_time(7.0), "0:07");
    assert_eq!(format_time(83.0), "1:23");
    assert_eq!(format_time(3599.0), "59:59");
    assert_eq!(format_time(3600.0), "1:00:00");
    assert_eq!(format_time(3723.0), "1:02:03");
}

#[test]
fn tempo_invalido_nao_quebra_o_relogio() {
    assert_eq!(format_time(-5.0), "0:00");
    assert_eq!(format_time(f64::NAN), "0:00");
    assert_eq!(format_time(f64::INFINITY), "0:00");
}

#[test]
fn selo_compacto_usa_bits_barra_kilohertz() {
    assert_eq!(compact_quality(44_100.0, 24), "24/44.1");
    assert_eq!(compact_quality(96_000.0, 24), "24/96");
    assert_eq!(compact_quality(192_000.0, 24), "24/192");
    assert_eq!(compact_quality(44_100.0, 16), "16/44.1");
}

#[test]
fn detalhe_de_qualidade_mostra_a_casa_decimal_so_quando_existe() {
    assert_eq!(
        quality_detail(44_100.0, 24, "flac"),
        "44.1 kHz · 24 bit · flac"
    );
    assert_eq!(
        quality_detail(96_000.0, 24, "flac"),
        "96 kHz · 24 bit · flac"
    );
}
