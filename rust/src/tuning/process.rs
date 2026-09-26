//! Aplica o filtro ao sinal, por convolução rápida com sobreposição e soma.
//!
//! Roda na thread que decodifica, nunca no IOProc: é por isso que pode alocar na construção e
//! usar FFT sem quebrar a disciplina de tempo real. O ring buffer entre as duas absorve a
//! irregularidade de processar em blocos.
//!
//! Trabalha em `f64` por dentro mesmo recebendo `f32`: a soma de um bloco de 4096 termos com o
//! espectro do filtro acumula erro, e 24 bits de mantissa não sobram tanto assim depois de uma
//! ida e volta pelo domínio da frequência.

use crate::tuning::design::Fir;
use crate::tuning::fft::{Complex, forward, inverse};

/// Amostras acima disto não existem no destino: o DAC recebe ±1,0 como fundo de escala, e o
/// que passa não fica mais alto — vira distorção grosseira.
const FULL_SCALE: f64 = 1.0;

pub struct Processor {
    /// Espectro do filtro, já no tamanho da FFT de trabalho.
    kernel: Vec<Complex>,
    /// Cauda do bloco anterior, por canal, que ainda precisa ser somada ao próximo.
    overlap: Vec<Vec<f64>>,
    scratch: Vec<Complex>,
    taps: usize,
    channels: usize,
    max_block_frames: usize,
    clipped: u64,
}

impl Processor {
    /// # Errors
    ///
    /// Se o número de canais for zero ou o bloco máximo não couber junto com o filtro.
    pub fn new(fir: &Fir, channels: usize, max_block_frames: usize) -> Result<Self, String> {
        if channels == 0 {
            return Err("processador sem canais".to_string());
        }
        if max_block_frames == 0 {
            return Err("bloco máximo de zero frames".to_string());
        }

        let taps = fir.len();
        let mut fft_size = 1;
        while fft_size < max_block_frames + taps - 1 {
            fft_size <<= 1;
        }

        let mut kernel = vec![Complex::ZERO; fft_size];
        for (slot, &tap) in kernel.iter_mut().zip(fir.taps()) {
            *slot = Complex::real(tap);
        }
        forward(&mut kernel);

        Ok(Self {
            kernel,
            overlap: vec![vec![0.0; taps.saturating_sub(1)]; channels],
            scratch: vec![Complex::ZERO; fft_size],
            taps,
            channels,
            max_block_frames,
            clipped: 0,
        })
    }

    /// Quantas amostras chegaram ao limite da escala e foram ceifadas. Zero é o esperado com a
    /// curva normalizada; qualquer outro número significa que o preamp não deu conta do
    /// material e que o resultado saiu distorcido, não apenas alto.
    pub fn clipped(&self) -> u64 {
        self.clipped
    }

    pub fn max_block_frames(&self) -> usize {
        self.max_block_frames
    }

    /// Filtra `samples` no lugar. O buffer é intercalado e tem de conter frames inteiros.
    ///
    /// O estado entre chamadas é a cauda do bloco anterior: chamar isto para blocos de uma
    /// faixa, em ordem, produz o mesmo resultado que filtrar a faixa inteira de uma vez.
    ///
    /// # Errors
    ///
    /// Se o buffer não for múltiplo dos canais ou exceder o bloco máximo declarado.
    pub fn process(&mut self, samples: &mut [f32]) -> Result<(), String> {
        if !samples.len().is_multiple_of(self.channels) {
            return Err(format!(
                "buffer de {} amostras não divide entre {} canais",
                samples.len(),
                self.channels
            ));
        }
        let frames = samples.len() / self.channels;
        if frames > self.max_block_frames {
            return Err(format!(
                "bloco de {frames} frames excede o máximo de {}",
                self.max_block_frames
            ));
        }
        if frames == 0 {
            return Ok(());
        }

        for channel in 0..self.channels {
            self.process_channel(samples, channel, frames);
        }
        Ok(())
    }

    fn process_channel(&mut self, samples: &mut [f32], channel: usize, frames: usize) {
        for (i, slot) in self.scratch.iter_mut().enumerate() {
            *slot = if i < frames {
                Complex::real(samples[i * self.channels + channel] as f64)
            } else {
                Complex::ZERO
            };
        }

        forward(&mut self.scratch);
        for (value, k) in self.scratch.iter_mut().zip(self.kernel.iter()) {
            let re = value.re * k.re - value.im * k.im;
            let im = value.re * k.im + value.im * k.re;
            *value = Complex::new(re, im);
        }
        inverse(&mut self.scratch);

        let tail = self.taps - 1;
        let overlap = &mut self.overlap[channel];
        for frame in 0..frames {
            let value = self.scratch[frame].re + overlap.get(frame).copied().unwrap_or(0.0);
            let limited = value.clamp(-FULL_SCALE, FULL_SCALE);
            if limited != value {
                self.clipped += 1;
            }
            samples[frame * self.channels + channel] = limited as f32;
        }

        // A cauda do bloco atual, mais o que sobrou da cauda anterior sem ser consumido — o que
        // acontece quando o bloco é mais curto que o filtro, no fim da faixa.
        let mut next = vec![0.0; tail];
        for (j, slot) in next.iter_mut().enumerate() {
            let carry = if frames + j < tail {
                overlap[frames + j]
            } else {
                0.0
            };
            *slot = self.scratch[frames + j].re + carry;
        }
        *overlap = next;
    }

    /// Quantos frames de cauda o filtro ainda tem para entregar depois do último bloco. Parar
    /// sem drenar corta o fim da faixa — não o silêncio depois dela, mas a reverberação do
    /// filtro sobre as últimas notas.
    pub fn tail_frames(&self) -> usize {
        self.taps - 1
    }

    /// Escoa a cauda para `samples`, que é preenchido inteiro. Depois disto o processador está
    /// zerado e pronto para outra faixa.
    pub fn drain_tail(&mut self, samples: &mut [f32]) {
        let frames = samples.len() / self.channels;
        for channel in 0..self.channels {
            let overlap = &self.overlap[channel];
            for frame in 0..frames {
                let value = overlap.get(frame).copied().unwrap_or(0.0);
                samples[frame * self.channels + channel] = value as f32;
            }
        }
        for overlap in &mut self.overlap {
            overlap.fill(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::curve::parse;
    use crate::tuning::design::{DEFAULT_TAPS, design};

    /// Convolução escrita da definição, sobre o sinal inteiro de uma vez. É o oráculo: lenta,
    /// sem sobreposição, sem FFT — nada que ela compartilhe com o código sob teste.
    fn convolucao_direta(sinal: &[f64], taps: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; sinal.len()];
        for (n, slot) in out.iter_mut().enumerate() {
            let mut sum = 0.0;
            for (k, &tap) in taps.iter().enumerate() {
                if n >= k {
                    sum += tap * sinal[n - k];
                }
            }
            *slot = sum;
        }
        out
    }

    fn ruido(n: usize) -> Vec<f32> {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            out.push(((state >> 40) as f64 / (1u64 << 24) as f64 - 0.5) as f32 * 0.5);
        }
        out
    }

    fn fir(taps: Vec<f64>) -> Fir {
        Fir::from_taps(taps, 48000.0)
    }

    fn dourada() -> Fir {
        let text = std::fs::read_to_string("../curves/dourada.curve").expect("curva versionada");
        let curva = parse(&text).expect("deveria parsear").normalized();
        design(&curva, 96000.0, DEFAULT_TAPS).expect("desenho")
    }

    #[test]
    fn filtro_delta_devolve_o_sinal_intacto() {
        let mut p = Processor::new(&fir(vec![1.0, 0.0, 0.0, 0.0]), 2, 64).expect("processador");
        let entrada = ruido(128);
        let mut samples = entrada.clone();

        p.process(&mut samples).expect("deveria processar");

        for (i, (obtido, esperado)) in samples.iter().zip(entrada.iter()).enumerate() {
            assert!((obtido - esperado).abs() < 1e-6, "amostra {i}");
        }
    }

    #[test]
    fn bate_com_a_convolucao_direta() {
        let taps: Vec<f64> = (0..16).map(|k| 1.0 / (1.0 + k as f64)).collect();
        let mut p = Processor::new(&fir(taps.clone()), 1, 256).expect("processador");
        let entrada = ruido(256);
        let esperado = convolucao_direta(
            &entrada.iter().map(|&v| v as f64).collect::<Vec<_>>(),
            &taps,
        );

        let mut samples = entrada.clone();
        p.process(&mut samples).expect("deveria processar");

        for (i, (obtido, esperado)) in samples.iter().zip(esperado.iter()).enumerate() {
            assert!(
                (*obtido as f64 - esperado).abs() < 1e-6,
                "amostra {i}: {obtido} != {esperado}"
            );
        }
    }

    /// O contrato que a sobreposição e soma existe para cumprir: partir o sinal em blocos não
    /// pode mudar o resultado. Uma emenda errada aparece como estalo na fronteira do bloco, que
    /// é audível e não aparece em nenhum teste de bloco único.
    #[test]
    fn processar_em_blocos_e_igual_a_processar_de_uma_vez() {
        let taps: Vec<f64> = (0..64).map(|k| (-(k as f64) / 20.0).exp()).collect();
        let entrada = ruido(1000);

        let mut inteiro = entrada.clone();
        Processor::new(&fir(taps.clone()), 1, 1000)
            .expect("processador")
            .process(&mut inteiro)
            .expect("deveria processar");

        let mut em_blocos = entrada.clone();
        let mut p = Processor::new(&fir(taps), 1, 128).expect("processador");
        for bloco in em_blocos.chunks_mut(37) {
            p.process(bloco).expect("deveria processar");
        }

        for (i, (a, b)) in em_blocos.iter().zip(inteiro.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "amostra {i}: {a} != {b}");
        }
    }

    /// Blocos menores que o filtro acontecem no fim da faixa. Aí a cauda anterior sobra sem ser
    /// consumida inteira, e é preciso arrastá-la — esquecer isso corta o fim de cada bloco.
    #[test]
    fn blocos_menores_que_o_filtro_arrastam_a_cauda() {
        let taps: Vec<f64> = (0..32).map(|k| (-(k as f64) / 8.0).exp()).collect();
        let entrada = ruido(200);

        let mut inteiro = entrada.clone();
        Processor::new(&fir(taps.clone()), 1, 200)
            .expect("processador")
            .process(&mut inteiro)
            .expect("deveria processar");

        let mut picado = entrada.clone();
        let mut p = Processor::new(&fir(taps), 1, 8).expect("processador");
        for bloco in picado.chunks_mut(5) {
            p.process(bloco).expect("deveria processar");
        }

        for (i, (a, b)) in picado.iter().zip(inteiro.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "amostra {i}: {a} != {b}");
        }
    }

    #[test]
    fn os_canais_nao_vazam_um_no_outro() {
        let mut p = Processor::new(&fir(vec![1.0, 0.5, 0.25]), 2, 64).expect("processador");

        // Esquerdo com sinal, direito em silêncio: qualquer coisa que apareça à direita veio
        // do canal errado.
        let mut samples = vec![0.0f32; 64];
        for frame in 0..32 {
            samples[frame * 2] = 0.5;
        }

        p.process(&mut samples).expect("deveria processar");

        for frame in 0..32 {
            assert!(
                samples[frame * 2].abs() > 0.0,
                "esquerdo mudo no frame {frame}"
            );
            assert_eq!(samples[frame * 2 + 1], 0.0, "direito sujo no frame {frame}");
        }
    }

    #[test]
    fn amostra_acima_da_escala_e_ceifada_e_contada() {
        // Ganho de 2x com um sinal já em meia escala: o resultado passa de 1,0 e não cabe.
        let mut p = Processor::new(&fir(vec![2.0]), 1, 16).expect("processador");
        let mut samples = vec![0.9f32; 16];

        p.process(&mut samples).expect("deveria processar");

        assert_eq!(p.clipped(), 16);
        for value in &samples {
            assert!((*value - 1.0).abs() < 1e-6, "ceifado em {value}");
        }
    }

    #[test]
    fn sinal_dentro_da_escala_nao_conta_ceifa() {
        let mut p = Processor::new(&dourada(), 2, 512).expect("processador");
        let mut samples = ruido(1024);

        p.process(&mut samples).expect("deveria processar");

        assert_eq!(p.clipped(), 0);
    }

    #[test]
    fn buffer_incompativel_e_recusado() {
        let mut p = Processor::new(&fir(vec![1.0]), 2, 64).expect("processador");

        assert!(p.process(&mut [0.0; 7]).is_err()); // não divide entre 2 canais
        assert!(p.process(&mut [0.0; 400]).is_err()); // 200 frames > 64
    }

    #[test]
    fn processador_sem_canais_e_recusado() {
        assert!(Processor::new(&fir(vec![1.0]), 0, 64).is_err());
        assert!(Processor::new(&fir(vec![1.0]), 2, 0).is_err());
    }

    /// A cauda é a resposta do filtro às últimas amostras. Descartá-la corta o fim da faixa.
    #[test]
    fn a_cauda_escoada_completa_a_convolucao() {
        let taps: Vec<f64> = (0..16).map(|k| 1.0 / (1.0 + k as f64)).collect();
        let entrada = ruido(64);

        // O oráculo: o mesmo sinal seguido de silêncio, convoluído de uma vez. O trecho de
        // silêncio é onde a cauda do filtro aparece.
        let mut estendido: Vec<f64> = entrada.iter().map(|&v| v as f64).collect();
        estendido.extend(std::iter::repeat_n(0.0, taps.len() - 1));
        let esperado = convolucao_direta(&estendido, &taps);

        let mut p = Processor::new(&fir(taps.clone()), 1, 64).expect("processador");
        let mut samples = entrada.clone();
        p.process(&mut samples).expect("deveria processar");
        let mut cauda = vec![0.0f32; p.tail_frames()];
        p.drain_tail(&mut cauda);

        let obtido: Vec<f32> = samples.iter().chain(cauda.iter()).copied().collect();
        for (i, (a, b)) in obtido.iter().zip(esperado.iter()).enumerate() {
            assert!((*a as f64 - b).abs() < 1e-6, "amostra {i}: {a} != {b}");
        }
    }
}
