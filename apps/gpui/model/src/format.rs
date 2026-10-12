//! Relógio e qualidade: porte de `TimeFormat.swift` e `Quality.swift`. A linha técnica e o
//! tooltip do selo saem daqui, para nunca descreverem a mesma faixa de jeitos diferentes.

/// Negativo, NaN e infinito viram `0:00`: `total_seconds` chega zerado antes da carga, e taxa
/// zerada produz NaN.
pub fn format_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "0:00".to_string();
    }
    let total = seconds.floor() as u64;
    let (hours, minutes, secs) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

/// 44,1/88,2/176,4 kHz têm casa decimal real; 48/96/192 não. Suprimir o zero evita "96.0" sem
/// arredondar 44,1 para 44, que seria informação errada sobre o que chega ao DAC.
fn format_kilohertz(hz: f64) -> String {
    let khz = hz / 1000.0;
    if khz.fract() == 0.0 {
        format!("{khz:.0}")
    } else {
        format!("{khz:.1}")
    }
}

/// "44.1 kHz · 24 bit · flac" ou, na faixa com perda, "48 kHz · AAC 339 kbps".
pub fn quality_detail(
    sample_rate: f64,
    bit_depth: Option<u32>,
    bitrate_kbps: Option<u32>,
    codec: &str,
) -> String {
    let rate = format!("{} kHz", format_kilohertz(sample_rate));
    match (bit_depth, bitrate_kbps) {
        (Some(bits), _) => format!("{rate} · {bits} bit · {codec}"),
        (None, Some(kbps)) => format!("{rate} · {} {kbps} kbps", codec.to_uppercase()),
        (None, None) => format!("{rate} · {}", codec.to_uppercase()),
    }
}

/// "24/44.1" ou, na faixa com perda, que não tem bits a mostrar, "AAC 339".
pub fn compact_quality(
    sample_rate: f64,
    bit_depth: Option<u32>,
    bitrate_kbps: Option<u32>,
    codec: &str,
) -> String {
    match (bit_depth, bitrate_kbps) {
        (Some(bits), _) => format!("{bits}/{}", format_kilohertz(sample_rate)),
        (None, Some(kbps)) => format!("{} {kbps}", codec.to_uppercase()),
        (None, None) => codec.to_uppercase(),
    }
}
