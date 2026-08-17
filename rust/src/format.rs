//! Núcleo puro da decisão de formato. Nenhuma dependência de Core Audio, para que a regra
//! que determina se a reprodução é bit-perfect possa ser testada sem tocar em hardware.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleType {
    Integer,
    Float,
}

/// Um formato físico oferecido pelo device. `index` aponta para a posição do
/// AudioStreamBasicDescription correspondente na lista que a camada HAL enumerou: o núcleo
/// decide qual usar, a camada HAL sabe como aplicá-lo.
///
/// Interfaces profissionais publicam formatos com faixa contínua em vez de uma taxa fixa.
/// Quando `sample_rate_maximum` excede o mínimo, qualquer taxa dentro da faixa serve; com os
/// dois zerados, apenas `sample_rate` vale.
#[derive(Clone, Copy, Debug)]
pub struct PhysicalFormatDesc {
    pub index: i32,
    pub sample_rate: f64,
    pub sample_type: SampleType,
    pub bits_per_channel: u32,
    pub channels: u32,
    pub sample_rate_minimum: f64,
    pub sample_rate_maximum: f64,
}

/// Devices podem publicar rates discretos (minimum == maximum) ou faixas contínuas.
#[derive(Clone, Copy, Debug)]
pub struct SampleRateRange {
    pub minimum: f64,
    pub maximum: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct FileFormat {
    pub sample_rate: f64,
    pub bit_depth: u32,
    pub channels: u32,
}

#[derive(Clone, Debug, Default)]
pub struct DeviceCaps {
    pub rates: Vec<SampleRateRange>,
    pub physical_formats: Vec<PhysicalFormatDesc>,
    pub output_channels: u32,
}

#[derive(Clone, Debug)]
pub struct Decision {
    pub play: bool,
    pub sample_rate: f64,
    pub physical_format_index: i32,
    pub duplicate_mono_to_stereo: bool,
    pub reason: String,
}

impl Default for Decision {
    fn default() -> Self {
        Self {
            play: false,
            sample_rate: 0.0,
            physical_format_index: -1,
            duplicate_mono_to_stereo: false,
            reason: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FormatCheck {
    pub ok: bool,
    pub reason: String,
}

// Rates são valores inteiros na prática (44100, 96000...), separados por milhares.
fn same_rate(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.5
}

fn rate_text(rate: f64) -> String {
    format!("{}", rate.round() as i64)
}

fn supported_rates_text(rates: &[SampleRateRange]) -> String {
    let mut out = String::new();
    for r in rates {
        if !out.is_empty() {
            out.push_str(", ");
        }
        if same_rate(r.minimum, r.maximum) {
            out.push_str(&rate_text(r.minimum));
        } else {
            out.push_str(&format!("{}-{}", rate_text(r.minimum), rate_text(r.maximum)));
        }
    }
    if out.is_empty() {
        "nenhum".to_string()
    } else {
        out
    }
}

fn rate_is_supported(rate: f64, rates: &[SampleRateRange]) -> bool {
    rates
        .iter()
        .any(|r| rate >= r.minimum - 0.5 && rate <= r.maximum + 0.5)
}

// float32 tem 24 bits de mantissa: inteiros de até 24 bits sobrevivem à ida e volta sem
// perda. Acima disso a conversão deixaria de ser exata e o formato é recusado.
const FLOAT_EXACT_BITS: u32 = 24;

fn preserves_every_bit(fmt: &PhysicalFormatDesc, file_bit_depth: u32) -> bool {
    match fmt.sample_type {
        SampleType::Float => file_bit_depth <= FLOAT_EXACT_BITS,
        SampleType::Integer => fmt.bits_per_channel >= file_bit_depth,
    }
}

// Um formato serve à taxa pedida se a declara diretamente ou se publica uma faixa contínua
// que a contém — o caso das interfaces profissionais, que anunciam a faixa em vez de listar
// cada taxa.
fn format_covers_rate(fmt: &PhysicalFormatDesc, rate: f64) -> bool {
    if fmt.sample_rate_maximum > fmt.sample_rate_minimum {
        return rate >= fmt.sample_rate_minimum - 0.5 && rate <= fmt.sample_rate_maximum + 0.5;
    }
    same_rate(fmt.sample_rate, rate)
}

// Entre os formatos íntegros: inteiro ganha de float, e o menor bit depth suficiente ganha
// dos maiores — trocar bits de largura não perde informação, mas o menor é o mais direto.
fn is_better(candidate: &PhysicalFormatDesc, current: &PhysicalFormatDesc) -> bool {
    if candidate.sample_type != current.sample_type {
        return candidate.sample_type == SampleType::Integer;
    }
    candidate.bits_per_channel < current.bits_per_channel
}

/// Decide se o arquivo pode ser reproduzido sem resample e sem perda de bits, e com qual
/// formato físico. Nunca escolhe um formato que exija conversão destrutiva: quando não há
/// caminho íntegro, devolve `play == false` com o motivo preenchido.
pub fn negotiate(file: &FileFormat, caps: &DeviceCaps) -> Decision {
    let mut d = Decision::default();

    if file.channels == 0 {
        d.reason = "arquivo sem canais de áudio".to_string();
        return d;
    }
    if file.channels > caps.output_channels {
        d.reason = format!(
            "arquivo tem {} canais; o device oferece {}",
            file.channels, caps.output_channels
        );
        return d;
    }

    if !rate_is_supported(file.sample_rate, &caps.rates) {
        d.reason = format!(
            "arquivo em {} Hz; o device suporta: {}. Reproduzir exigiria resample.",
            rate_text(file.sample_rate),
            supported_rates_text(&caps.rates)
        );
        return d;
    }

    let mut best: Option<&PhysicalFormatDesc> = None;
    for fmt in &caps.physical_formats {
        if !format_covers_rate(fmt, file.sample_rate) {
            continue;
        }
        if !preserves_every_bit(fmt, file.bit_depth) {
            continue;
        }
        best = match best {
            None => Some(fmt),
            Some(current) if is_better(fmt, current) => Some(fmt),
            other => other,
        };
    }

    let Some(best) = best else {
        d.reason = format!(
            "nenhum formato físico do device em {} Hz comporta {} bits sem perda",
            rate_text(file.sample_rate),
            file.bit_depth
        );
        return d;
    };

    d.play = true;
    d.sample_rate = file.sample_rate;
    d.physical_format_index = best.index;
    d.duplicate_mono_to_stereo = file.channels == 1 && caps.output_channels >= 2;
    d.reason = format!(
        "{} Hz / {} bits {}",
        rate_text(file.sample_rate),
        best.bits_per_channel,
        if best.sample_type == SampleType::Float {
            "float"
        } else {
            "inteiro"
        }
    );
    d
}

/// Confere se um formato intercalado é internamente consistente antes de alimentar o device.
/// Um descasamento entre o que o decodificador produz e o que o DAC espera não soa como
/// distorção leve: soa como ruído branco em volume total.
pub fn validate_interleaved_format(
    bits_per_channel: u32,
    bytes_per_frame: u32,
    channels: u32,
) -> FormatCheck {
    let fail = |reason: String| FormatCheck { ok: false, reason };

    if channels == 0 {
        return fail("formato sem canais".to_string());
    }
    if bits_per_channel == 0 {
        return fail("formato sem profundidade de bits".to_string());
    }
    if bytes_per_frame == 0 {
        return fail("formato sem tamanho de frame".to_string());
    }
    if bits_per_channel % 8 != 0 {
        return fail(format!(
            "profundidade de {bits_per_channel} bits não é múltipla de 8; o layout no buffer seria ambíguo"
        ));
    }
    if bytes_per_frame % channels != 0 {
        return fail(format!(
            "frame de {bytes_per_frame} bytes não divide entre {channels} canais"
        ));
    }

    // O container por canal pode ser maior que a amostra (int24 dentro de 32 bits), nunca menor.
    let bytes_per_channel = bytes_per_frame / channels;
    if bytes_per_channel * 8 < bits_per_channel {
        return fail(format!(
            "container de {bytes_per_channel} bytes por canal não comporta amostras de {bits_per_channel} bits"
        ));
    }

    FormatCheck {
        ok: true,
        reason: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int_format(index: i32, rate: f64, bits: u32) -> PhysicalFormatDesc {
        PhysicalFormatDesc {
            index,
            sample_rate: rate,
            sample_type: SampleType::Integer,
            bits_per_channel: bits,
            channels: 2,
            sample_rate_minimum: 0.0,
            sample_rate_maximum: 0.0,
        }
    }

    fn float_format(index: i32, rate: f64) -> PhysicalFormatDesc {
        PhysicalFormatDesc {
            index,
            sample_rate: rate,
            sample_type: SampleType::Float,
            bits_per_channel: 32,
            channels: 2,
            sample_rate_minimum: 0.0,
            sample_rate_maximum: 0.0,
        }
    }

    fn discrete(rate: f64) -> SampleRateRange {
        SampleRateRange {
            minimum: rate,
            maximum: rate,
        }
    }

    // Um DAC built-in típico do Apple Silicon: rates discretos, 24-bit inteiro em cada um.
    fn built_in_like() -> DeviceCaps {
        DeviceCaps {
            rates: vec![
                discrete(44100.0),
                discrete(48000.0),
                discrete(88200.0),
                discrete(96000.0),
            ],
            physical_formats: vec![
                int_format(0, 44100.0, 24),
                int_format(1, 48000.0, 24),
                int_format(2, 88200.0, 24),
                int_format(3, 96000.0, 24),
            ],
            output_channels: 2,
        }
    }

    fn file(rate: f64, bits: u32, channels: u32) -> FileFormat {
        FileFormat {
            sample_rate: rate,
            bit_depth: bits,
            channels,
        }
    }

    #[test]
    fn rate_exatamente_suportado_e_aceito() {
        let d = negotiate(&file(96000.0, 24, 2), &built_in_like());

        assert!(d.play);
        assert_eq!(d.sample_rate, 96000.0);
        assert_eq!(d.physical_format_index, 3);
        assert!(!d.duplicate_mono_to_stereo);
    }

    #[test]
    fn rate_dentro_de_range_continuo_e_aceito() {
        let caps = DeviceCaps {
            rates: vec![SampleRateRange {
                minimum: 32000.0,
                maximum: 192000.0,
            }],
            physical_formats: vec![int_format(0, 176400.0, 24)],
            output_channels: 2,
        };

        let d = negotiate(&file(176400.0, 24, 2), &caps);

        assert!(d.play);
        assert_eq!(d.sample_rate, 176400.0);
    }

    // Interfaces profissionais publicam o formato físico com faixa contínua em vez de uma taxa
    // fixa. Recusar só porque a taxa do arquivo não é o extremo da faixa seria um falso negativo.
    #[test]
    fn formato_fisico_de_faixa_continua_aceita_taxa_interna() {
        let mut continuo = int_format(0, 44100.0, 24);
        continuo.sample_rate_minimum = 44100.0;
        continuo.sample_rate_maximum = 192000.0;
        let caps = DeviceCaps {
            rates: vec![SampleRateRange {
                minimum: 44100.0,
                maximum: 192000.0,
            }],
            physical_formats: vec![continuo],
            output_channels: 2,
        };

        let d = negotiate(&file(96000.0, 24, 2), &caps);

        assert!(d.play);
        assert_eq!(d.sample_rate, 96000.0);
        assert_eq!(d.physical_format_index, 0);
    }

    #[test]
    fn formato_fisico_de_faixa_continua_recusa_taxa_fora_da_faixa() {
        let mut continuo = int_format(0, 44100.0, 24);
        continuo.sample_rate_minimum = 44100.0;
        continuo.sample_rate_maximum = 96000.0;
        let caps = DeviceCaps {
            rates: vec![SampleRateRange {
                minimum: 44100.0,
                maximum: 192000.0,
            }],
            physical_formats: vec![continuo],
            output_channels: 2,
        };

        let d = negotiate(&file(192000.0, 24, 2), &caps);

        assert!(!d.play);
    }

    #[test]
    fn rate_nao_suportado_aborta() {
        let d = negotiate(&file(192000.0, 24, 2), &built_in_like());

        assert!(!d.play);
        assert_eq!(d.physical_format_index, -1);
        assert!(d.reason.contains("192000"));
        assert!(d.reason.contains("96000"));
    }

    #[test]
    fn arquivo_16_bits_usa_formato_de_24_quando_e_o_unico() {
        let d = negotiate(&file(44100.0, 16, 2), &built_in_like());

        assert!(d.play);
        assert_eq!(d.physical_format_index, 0);
    }

    #[test]
    fn escolhe_o_menor_bit_depth_que_comporta_o_arquivo() {
        let caps = DeviceCaps {
            rates: vec![discrete(44100.0)],
            physical_formats: vec![
                int_format(0, 44100.0, 32),
                int_format(1, 44100.0, 16),
                int_format(2, 44100.0, 24),
            ],
            output_channels: 2,
        };

        let d = negotiate(&file(44100.0, 24, 2), &caps);

        assert!(d.play);
        // 24 comporta 24; 16 não; 32 é maior que o necessário.
        assert_eq!(d.physical_format_index, 2);
    }

    #[test]
    fn nao_escolhe_bit_depth_menor_que_o_do_arquivo() {
        let caps = DeviceCaps {
            rates: vec![discrete(44100.0)],
            physical_formats: vec![int_format(0, 44100.0, 16)],
            output_channels: 2,
        };

        let d = negotiate(&file(44100.0, 24, 2), &caps);

        assert!(!d.play);
        assert!(d.reason.contains("24"));
    }

    #[test]
    fn float32_e_aceito_para_arquivo_de_ate_24_bits() {
        let caps = DeviceCaps {
            rates: vec![discrete(96000.0)],
            physical_formats: vec![float_format(0, 96000.0)],
            output_channels: 2,
        };

        let d = negotiate(&file(96000.0, 24, 2), &caps);

        assert!(d.play);
        assert_eq!(d.physical_format_index, 0);
    }

    // float32 tem 24 bits de mantissa: um inteiro de 32 bits não sobrevive à ida e volta.
    #[test]
    fn float32_nao_comporta_arquivo_de_32_bits_inteiros() {
        let caps = DeviceCaps {
            rates: vec![discrete(96000.0)],
            physical_formats: vec![float_format(0, 96000.0)],
            output_channels: 2,
        };

        let d = negotiate(&file(96000.0, 32, 2), &caps);

        assert!(!d.play);
        assert_eq!(d.physical_format_index, -1);
    }

    #[test]
    fn prefere_inteiro_a_float_no_mesmo_rate() {
        let caps = DeviceCaps {
            rates: vec![discrete(96000.0)],
            physical_formats: vec![float_format(0, 96000.0), int_format(1, 96000.0, 24)],
            output_channels: 2,
        };

        let d = negotiate(&file(96000.0, 24, 2), &caps);

        assert!(d.play);
        assert_eq!(d.physical_format_index, 1);
    }

    #[test]
    fn mono_e_duplicado_para_estereo() {
        let d = negotiate(&file(44100.0, 16, 1), &built_in_like());

        assert!(d.play);
        assert!(d.duplicate_mono_to_stereo);
    }

    #[test]
    fn mais_canais_que_o_device_aborta() {
        let d = negotiate(&file(48000.0, 24, 6), &built_in_like());

        assert!(!d.play);
        assert!(d.reason.contains('6'));
    }

    #[test]
    fn rate_suportado_mas_sem_formato_fisico_no_rate_aborta() {
        let caps = DeviceCaps {
            rates: vec![discrete(44100.0), discrete(96000.0)],
            physical_formats: vec![int_format(0, 44100.0, 24)],
            output_channels: 2,
        };

        let d = negotiate(&file(96000.0, 24, 2), &caps);

        assert!(!d.play);
        assert_eq!(d.physical_format_index, -1);
    }

    #[test]
    fn device_sem_formato_algum_aborta() {
        let caps = DeviceCaps {
            rates: vec![discrete(44100.0)],
            physical_formats: vec![],
            output_channels: 2,
        };

        assert!(!negotiate(&file(44100.0, 24, 2), &caps).play);
    }

    // --- validação do formato de entrega ---------------------------------------------
    // Um descasamento entre o que o decodificador produz e o que o device espera não degrada
    // o som: vira ruído branco em volume total, capaz de danificar fone e audição.

    #[test]
    fn formatos_consistentes_sao_aceitos() {
        assert!(validate_interleaved_format(32, 8, 2).ok); // float32 estéreo
        assert!(validate_interleaved_format(16, 4, 2).ok); // int16 estéreo
        assert!(validate_interleaved_format(24, 6, 2).ok); // int24 packed
        assert!(validate_interleaved_format(24, 8, 2).ok); // int24 em container de 32 bits
        assert!(validate_interleaved_format(32, 4, 1).ok); // float32 mono
    }

    #[test]
    fn container_menor_que_a_amostra_e_recusado() {
        // 32 bits por canal não cabem em 2 bytes por canal.
        let r = validate_interleaved_format(32, 4, 2);

        assert!(!r.ok);
        assert!(!r.reason.is_empty());
    }

    #[test]
    fn bytes_por_frame_indivisivel_pelos_canais_e_recusado() {
        assert!(!validate_interleaved_format(16, 5, 2).ok);
    }

    #[test]
    fn profundidade_fora_do_byte_e_recusada() {
        assert!(!validate_interleaved_format(20, 8, 2).ok);
    }

    #[test]
    fn formato_degenerado_e_recusado() {
        assert!(!validate_interleaved_format(16, 4, 0).ok);
        assert!(!validate_interleaved_format(0, 4, 2).ok);
        assert!(!validate_interleaved_format(16, 0, 2).ok);
    }
}
