//! Transforma a curva no filtro que a realiza: um FIR de **fase mínima**.
//!
//! Fase mínima, e não linear, por dois motivos. O primeiro é o pré-eco: um filtro de fase
//! linear com este relevo espalha energia *antes* do transiente, que é o artefato que se ouve
//! como um "sopro" à frente da batida. O segundo é a latência, que na fase linear é metade do
//! comprimento do filtro. Fase mínima concentra a energia no início do impulso e é o que todo
//! EQ analógico faz.
//!
//! O método é o cepstro real: sai-se do log da magnitude, dobra-se a parte causal e volta-se
//! pela exponencial complexa. A fase que aparece nesse caminho é a única compatível com a
//! magnitude pedida sem energia antes do instante zero.

use crate::tuning::curve::Curve;
use crate::tuning::fft::{Complex, forward, inverse, is_power_of_two};

/// Comprimento padrão do filtro. A 96 kHz são 43 ms de janela, o que resolve ~23 Hz — de
/// sobra para o relevo desta curva, cuja feição mais estreita, a ondulação perto de 15 kHz,
/// tem centenas de hertz de largura.
pub const DEFAULT_TAPS: usize = 4096;

/// Piso da magnitude antes do log, em −120 dB: `ln(0)` é infinito, e um único infinito
/// contamina o cepstro inteiro com NaN.
const MAGNITUDE_FLOOR: f64 = 1e-6;

/// Fração final do impulso sobre a qual a janela desce até zero. Cortar o impulso a seco faz
/// a resposta ondular em torno da curva pedida; a descida suave troca essa ondulação por um
/// borrão pequeno e monotônico.
const TAIL_WINDOW_FRACTION: f64 = 0.25;

#[derive(Clone, Debug)]
pub struct Fir {
    taps: Vec<f64>,
    sample_rate: f64,
}

impl Fir {
    /// Filtro a partir de coeficientes prontos. Existe para que quem testa a convolução possa
    /// usar um filtro que ele mesmo escolheu — um delta, um atraso puro — em vez de depender do
    /// desenho, que é justamente o que não deve entrar no oráculo do outro módulo.
    #[cfg(test)]
    pub(crate) fn from_taps(taps: Vec<f64>, sample_rate: f64) -> Self {
        Self { taps, sample_rate }
    }

    pub fn taps(&self) -> &[f64] {
        &self.taps
    }

    pub fn len(&self) -> usize {
        self.taps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.taps.is_empty()
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Resposta em `hz`, avaliada direto da definição — `H(e^{jω}) = Σ h[n]·e^{-jωn}`.
    ///
    /// Deliberadamente **não** usa a FFT que desenhou o filtro: um erro dentro dela se
    /// esconderia atrás de si mesmo se o oráculo fosse o mesmo caminho.
    pub fn response_db(&self, hz: f64) -> f64 {
        let omega = 2.0 * std::f64::consts::PI * hz / self.sample_rate;
        let mut sum = Complex::ZERO;
        for (n, &tap) in self.taps.iter().enumerate() {
            let angle = -omega * n as f64;
            sum.re += tap * angle.cos();
            sum.im += tap * angle.sin();
        }
        20.0 * sum.magnitude().max(MAGNITUDE_FLOOR).log10()
    }
}

/// Desenha o filtro que realiza `curve` em `sample_rate`.
///
/// A curva entra **como está**: normalizar é decisão de quem chama, e fazê-lo aqui em silêncio
/// esconderia o ganho positivo que satura em vez de recusá-lo.
pub fn design(curve: &Curve, sample_rate: f64, taps: usize) -> Result<Fir, String> {
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return Err(format!("sample rate inválido: {sample_rate}"));
    }
    if taps < 64 {
        return Err(format!(
            "filtro de {taps} taps é curto demais para a banda audível"
        ));
    }
    if !is_power_of_two(taps) {
        return Err(format!(
            "o comprimento do filtro precisa ser potência de dois; recebeu {taps}"
        ));
    }

    // A grade do desenho é bem mais fina que o filtro: assim o truncamento age sobre um
    // impulso já correto, em vez de sobre uma curva mal amostrada no grave.
    let n = (taps * 8).max(8192);

    let mut spectrum = vec![Complex::ZERO; n];
    for (k, slot) in spectrum.iter_mut().enumerate() {
        // Acima de Nyquist o espectro espelha: é a mesma frequência, do outro lado.
        let bin = if k <= n / 2 { k } else { n - k };
        let hz = bin as f64 * sample_rate / n as f64;
        let magnitude = 10f64.powf(curve.gain_at(hz) / 20.0).max(MAGNITUDE_FLOOR);
        *slot = Complex::real(magnitude.ln());
    }

    // Cepstro real. Como o log da magnitude é real e par, o cepstro também é.
    inverse(&mut spectrum);

    // A dobra: mantém o instante zero e o de Nyquist, duplica a metade causal e zera a
    // anticausal. É esta operação, e só ela, que escolhe a fase mínima entre todas as fases
    // possíveis para a mesma magnitude.
    let half = n / 2;
    for value in &mut spectrum[1..half] {
        value.re *= 2.0;
        value.im *= 2.0;
    }
    for value in &mut spectrum[half + 1..] {
        *value = Complex::ZERO;
    }

    forward(&mut spectrum);
    for value in &mut spectrum {
        *value = value.exp();
    }
    inverse(&mut spectrum);

    let mut impulse: Vec<f64> = spectrum[..taps].iter().map(|c| c.re).collect();
    apply_tail_window(&mut impulse);

    Ok(Fir {
        taps: impulse,
        sample_rate,
    })
}

/// Meia janela de Hann sobre a cauda. O começo do impulso — onde a energia da fase mínima
/// está — fica intocado.
fn apply_tail_window(impulse: &mut [f64]) {
    let n = impulse.len();
    let tail = (n as f64 * TAIL_WINDOW_FRACTION) as usize;
    if tail < 2 {
        return;
    }
    let start = n - tail;
    for (i, tap) in impulse[start..].iter_mut().enumerate() {
        let t = i as f64 / (tail - 1) as f64;
        *tap *= 0.5 * (1.0 + (std::f64::consts::PI * t).cos());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::curve::{CurvePoint, parse};

    fn curve(points: &[(f64, f64)]) -> Curve {
        let points = points
            .iter()
            .map(|&(hz, db)| CurvePoint { hz, db })
            .collect();
        Curve::from_points(points).expect("curva do teste deveria ser válida")
    }

    fn dourada() -> Curve {
        let text = std::fs::read_to_string("../curves/dourada.curve").expect("curva versionada");
        parse(&text).expect("deveria parsear")
    }

    /// Terços de oitava de 25 Hz a 16 kHz: a grade em que se lê uma resposta de fone.
    fn tercos_de_oitava() -> Vec<f64> {
        let mut out = Vec::new();
        let mut hz = 25.0;
        while hz <= 16000.0 {
            out.push(hz);
            hz *= 2f64.powf(1.0 / 3.0);
        }
        out
    }

    fn pior_erro(fir: &Fir, curve: &Curve, freqs: &[f64]) -> (f64, f64) {
        let mut worst = 0.0;
        let mut onde = 0.0;
        for &hz in freqs {
            let erro = (fir.response_db(hz) - curve.gain_at(hz)).abs();
            if erro > worst {
                worst = erro;
                onde = hz;
            }
        }
        (worst, onde)
    }

    // O caso em que a resposta se sabe de cabeça: curva plana em 0 dB tem de virar um impulso
    // unitário, porque o filtro que não muda nada é o que copia a entrada.
    #[test]
    fn curva_plana_vira_um_impulso_unitario() {
        let fir = design(&curve(&[(20.0, 0.0), (20000.0, 0.0)]), 48000.0, 256).expect("desenho");

        assert!(
            (fir.taps()[0] - 1.0).abs() < 1e-6,
            "h[0] = {}",
            fir.taps()[0]
        );
        for (n, &tap) in fir.taps().iter().enumerate().skip(1) {
            assert!(tap.abs() < 1e-6, "h[{n}] = {tap} deveria ser zero");
        }
    }

    #[test]
    fn ganho_constante_e_realizado_exatamente() {
        let fir = design(&curve(&[(20.0, -6.0), (20000.0, -6.0)]), 48000.0, 256).expect("desenho");

        for hz in [100.0, 1000.0, 10000.0] {
            assert!((fir.response_db(hz) - -6.0).abs() < 0.01, "em {hz} Hz");
        }
    }

    /// O teste que importa: o filtro realmente tem a resposta que a curva pede.
    #[test]
    fn a_dourada_e_realizada_dentro_de_meio_db() {
        let curva = dourada().normalized();

        for rate in [44100.0, 48000.0, 96000.0] {
            let fir = design(&curva, rate, DEFAULT_TAPS).expect("desenho");
            let (erro, onde) = pior_erro(&fir, &curva, &tercos_de_oitava());

            assert!(
                erro < 0.1,
                "{rate} Hz: erro de {erro:.3} dB em {onde:.0} Hz"
            );
        }
    }

    /// O relevo é o que se ouve. Errar o ganho absoluto é volume; errar a distância entre o
    /// grave e o médio é outro timbre.
    #[test]
    fn o_relevo_do_grave_e_da_presenca_sobrevive_ao_desenho() {
        let curva = dourada().normalized();
        let fir = design(&curva, 96000.0, DEFAULT_TAPS).expect("desenho");

        let grave = fir.response_db(30.0) - fir.response_db(1000.0);
        let presenca = fir.response_db(3200.0) - fir.response_db(1000.0);

        assert!((grave - 13.0).abs() < 0.5, "grave: {grave:.2} dB");
        assert!((presenca - 10.6).abs() < 0.5, "presença: {presenca:.2} dB");
    }

    /// O mesmo filtro desenhado com fase zero: a alternativa ingênua, que é o que sai quando
    /// se ignora o cepstro e se faz a IFFT da magnitude direto. Existe aqui como oráculo — sem
    /// ele, "a energia está na frente" passa até com a fase destruída, porque o truncamento
    /// sozinho já concentra o que sobrou.
    fn fir_de_fase_linear(curve: &Curve, sample_rate: f64, taps: usize) -> Vec<f64> {
        let n = (taps * 8).max(8192);
        let mut spectrum = vec![Complex::ZERO; n];
        for (k, slot) in spectrum.iter_mut().enumerate() {
            let bin = if k <= n / 2 { k } else { n - k };
            let hz = bin as f64 * sample_rate / n as f64;
            *slot = Complex::real(10f64.powf(curve.gain_at(hz) / 20.0));
        }
        inverse(&mut spectrum);

        // Impulso simétrico em torno do instante zero: a metade anterior mora no fim do buffer,
        // e é preciso trazê-la para a frente para que o filtro seja realizável.
        let half = taps / 2;
        let mut out = Vec::with_capacity(taps);
        for i in 0..taps {
            out.push(spectrum[(n + i - half) % n].re);
        }
        out
    }

    fn energia_na_frente(taps: &[f64]) -> f64 {
        let total: f64 = taps.iter().map(|t| t * t).sum();
        let inicio: f64 = taps[..taps.len() / 10].iter().map(|t| t * t).sum();
        inicio / total
    }

    /// A razão de ser da fase mínima: a energia do impulso fica toda na frente, sem pré-eco.
    /// O contraste com o filtro de fase linear da mesma curva é o que dá sentido ao número —
    /// nele o pico fica no meio do impulso, e o transiente ganha um sopro antes da batida.
    #[test]
    fn a_fase_minima_concentra_a_energia_que_a_fase_linear_espalha() {
        let curva = dourada().normalized();
        let minima = design(&curva, 96000.0, DEFAULT_TAPS).expect("desenho");
        let linear = fir_de_fase_linear(&curva, 96000.0, DEFAULT_TAPS);

        let frente_minima = energia_na_frente(minima.taps());
        let frente_linear = energia_na_frente(&linear);

        assert!(
            frente_minima > 0.95,
            "fase mínima: {:.1}%",
            100.0 * frente_minima
        );
        assert!(
            frente_linear < 0.20,
            "fase linear: {:.1}%",
            100.0 * frente_linear
        );
    }

    #[test]
    fn o_impulso_nao_tem_nan_nem_infinito() {
        let fir = design(&dourada().normalized(), 44100.0, DEFAULT_TAPS).expect("desenho");

        for (n, &tap) in fir.taps().iter().enumerate() {
            assert!(tap.is_finite(), "h[{n}] = {tap}");
        }
    }

    #[test]
    fn parametros_invalidos_sao_recusados() {
        let c = curve(&[(20.0, 0.0), (20000.0, 0.0)]);

        assert!(design(&c, 0.0, 256).is_err());
        assert!(design(&c, -48000.0, 256).is_err());
        assert!(design(&c, 48000.0, 32).is_err()); // curto demais
        assert!(design(&c, 48000.0, 1000).is_err()); // não é potência de dois
    }
}
