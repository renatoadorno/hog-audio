//! A cadeia da afinação de ponta a ponta: curva, filtro, convolução em blocos e requantização
//! para o formato do device.
//!
//! Mede a resposta do jeito que se mede um equalizador de verdade — senoide entra, senoide
//! sai, compara-se a amplitude — em vez de reusar a FFT que desenhou o filtro. Um erro dentro
//! dela se esconderia atrás de si mesmo.

use hog_audio::format::SampleType;
use hog_audio::tuning::curve::{Curve, parse};
use hog_audio::tuning::design::{DEFAULT_TAPS, design};
use hog_audio::tuning::process::Processor;
use hog_audio::tuning::quantize::{OutputFormat, Quantizer};

const SAMPLE_RATE: f64 = 48000.0;
const BLOCO: usize = 1024;
const AMPLITUDE: f32 = 0.5;

fn dourada() -> Curve {
    let text = std::fs::read_to_string("../curves/dourada.curve").expect("curva versionada");
    parse(&text).expect("deveria parsear").normalized()
}

fn int24() -> OutputFormat {
    OutputFormat {
        sample_type: SampleType::Integer,
        bits_per_channel: 24,
        bytes_per_sample: 4,
    }
}

fn ler_int24_em_container_de_32(bytes: &[u8]) -> f32 {
    let mut value = (bytes[0] as i32) | ((bytes[1] as i32) << 8) | ((bytes[2] as i32) << 16);
    if value & 0x80_0000 != 0 {
        value -= 1 << 24;
    }
    value as f32 / 8_388_608.0
}

fn rms(samples: &[f32]) -> f64 {
    let soma: f64 = samples.iter().map(|&v| (v as f64) * (v as f64)).sum();
    (soma / samples.len() as f64).sqrt()
}

/// Passa uma senoide de `hz` pela cadeia inteira e devolve o ganho medido, em dB.
fn ganho_medido(curva: &Curve, hz: f64) -> f64 {
    let fir = design(curva, SAMPLE_RATE, DEFAULT_TAPS).expect("desenho");
    let mut processor = Processor::new(&fir, 1, BLOCO).expect("processador");
    let mut quantizer = Quantizer::new(int24(), 42).expect("quantizador");

    // Longo o bastante para o filtro assentar antes da janela de medição: o começo carrega o
    // transiente da convolução, que não é a resposta em regime.
    let total = DEFAULT_TAPS * 4;
    let entrada: Vec<f32> = (0..total)
        .map(|n| {
            let fase = 2.0 * std::f64::consts::PI * hz * n as f64 / SAMPLE_RATE;
            AMPLITUDE * fase.sin() as f32
        })
        .collect();

    let mut saida = Vec::with_capacity(total);
    let mut bytes = vec![0u8; BLOCO * 4];
    for bloco in entrada.chunks(BLOCO) {
        let mut samples = bloco.to_vec();
        processor.process(&mut samples).expect("deveria filtrar");
        let escritos = quantizer
            .write(&samples, &mut bytes)
            .expect("deveria escrever");
        for amostra in bytes[..escritos].as_chunks::<4>().0 {
            saida.push(ler_int24_em_container_de_32(amostra));
        }
    }

    let janela = DEFAULT_TAPS * 2;
    let medido = rms(&saida[total - janela..]);
    let referencia = rms(&entrada[total - janela..]);
    20.0 * (medido / referencia).log10()
}

/// O teste que fecha a afinação: o que sai no formato do DAC tem a resposta que a curva pede.
#[test]
fn a_cadeia_completa_realiza_a_curva_no_formato_do_device() {
    let curva = dourada();

    for hz in [50.0, 120.0, 500.0, 1000.0, 2000.0, 3200.0, 8000.0, 15000.0] {
        let esperado = curva.gain_at(hz);
        let obtido = ganho_medido(&curva, hz);

        assert!(
            (obtido - esperado).abs() < 0.5,
            "{hz} Hz: {obtido:.2} dB medido contra {esperado:.2} dB pedido"
        );
    }
}

/// O relevo entre as bandas é o que se ouve como timbre. Sobrevive à requantização inteira.
#[test]
fn o_relevo_sobrevive_a_cadeia_inteira() {
    let curva = dourada();

    let grave = ganho_medido(&curva, 50.0);
    let medio = ganho_medido(&curva, 1000.0);
    let presenca = ganho_medido(&curva, 3200.0);

    assert!(
        (grave - medio - 11.9).abs() < 0.5,
        "grave sobre o médio: {:.2} dB",
        grave - medio
    );
    assert!(
        (presenca - medio - 10.6).abs() < 0.5,
        "presença sobre o médio: {:.2} dB",
        presenca - medio
    );
}

/// Sem afinação nada disto acontece: é o contraste que dá sentido ao resto. Uma curva plana
/// atravessa a cadeia inteira e volta igual, dentro do erro de um LSB de 24 bits.
#[test]
fn curva_plana_atravessa_a_cadeia_sem_alterar_o_sinal() {
    let plana = Curve::from_points(vec![
        hog_audio::tuning::curve::CurvePoint { hz: 20.0, db: 0.0 },
        hog_audio::tuning::curve::CurvePoint {
            hz: 20000.0,
            db: 0.0,
        },
    ])
    .expect("curva plana");

    for hz in [100.0, 1000.0, 10000.0] {
        let ganho = ganho_medido(&plana, hz);
        assert!(ganho.abs() < 0.01, "{hz} Hz: {ganho:.4} dB");
    }
}
