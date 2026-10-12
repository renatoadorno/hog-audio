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
    assert_eq!(compact_quality(44_100.0, Some(24), None, "flac"), "24/44.1");
    assert_eq!(compact_quality(96_000.0, Some(24), None, "flac"), "24/96");
    assert_eq!(compact_quality(192_000.0, Some(24), None, "flac"), "24/192");
    assert_eq!(compact_quality(44_100.0, Some(16), None, "flac"), "16/44.1");
}

#[test]
fn detalhe_de_qualidade_mostra_a_casa_decimal_so_quando_existe() {
    assert_eq!(
        quality_detail(44_100.0, Some(24), None, "flac"),
        "44.1 kHz · 24 bit · flac"
    );
    assert_eq!(
        quality_detail(96_000.0, Some(24), None, "flac"),
        "96 kHz · 24 bit · flac"
    );
}

#[test]
fn faixa_com_perda_mostra_codec_e_bitrate_no_lugar_dos_bits() {
    assert_eq!(compact_quality(48_000.0, None, Some(339), "aac"), "AAC 339");
    assert_eq!(
        quality_detail(48_000.0, None, Some(339), "aac"),
        "48 kHz · AAC 339 kbps"
    );
}

#[test]
fn faixa_com_perda_sem_bitrate_mostra_so_o_codec() {
    assert_eq!(compact_quality(44_100.0, None, None, "mp3"), "MP3");
    assert_eq!(
        quality_detail(44_100.0, None, None, "mp3"),
        "44.1 kHz · MP3"
    );
}
