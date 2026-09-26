//! Converte as amostras filtradas para o formato que o device espera.
//!
//! No modo bit-perfect este módulo não existe no caminho: o decodificador já entrega no
//! formato do DAC e ninguém toca nos bytes. Ele só aparece quando a afinação está ligada, e
//! aí é a última etapa antes do ring buffer.
//!
//! Quando o destino é inteiro, a conversão adiciona **dither TPDF**. Truncar direto produz
//! distorção correlacionada com o sinal — audível justamente na passagem baixa, onde ela
//! aparece como aspereza em vez de chiado. O dither troca essa distorção por um ruído de
//! fundo constante, uns 3 dB acima do piso teórico e ainda muito abaixo do audível em 24 bits.

use crate::format::SampleType;

/// Layout de uma amostra no buffer do device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputFormat {
    pub sample_type: SampleType,
    pub bits_per_channel: u32,
    /// Tamanho do container por canal. Pode ser maior que a amostra — int24 costuma vir em
    /// quatro bytes — e nunca menor.
    pub bytes_per_sample: u32,
}

/// Gerador do dither. Xorshift porque precisa ser barato e reprodutível; a qualidade
/// estatística exigida de um dither de áudio é baixa perto da de um gerador criptográfico.
struct Noise {
    state: u64,
}

impl Noise {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x2545_F491_4F6C_DD1D
            } else {
                seed
            },
        }
    }

    /// Uniforme em [-0.5, 0.5).
    fn uniform(&mut self) -> f64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        (self.state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    }

    /// Triangular em [-1, 1) LSB: a soma de duas uniformes independentes. É a distribuição que
    /// desacopla tanto a média quanto a variância do erro do valor do sinal — uma uniforme só
    /// desacopla a média, e deixa o ruído "respirando" junto com a música.
    fn tpdf(&mut self) -> f64 {
        self.uniform() + self.uniform()
    }
}

pub struct Quantizer {
    format: OutputFormat,
    noise: Noise,
    clipped: u64,
}

impl Quantizer {
    /// # Errors
    ///
    /// Se o formato não for representável aqui: profundidade fora do byte, container menor que
    /// a amostra, ou float que não seja de 32 bits.
    pub fn new(format: OutputFormat, seed: u64) -> Result<Self, String> {
        if format.bits_per_channel == 0 || !format.bits_per_channel.is_multiple_of(8) {
            return Err(format!(
                "profundidade de {} bits não é múltipla de 8",
                format.bits_per_channel
            ));
        }
        if format.bytes_per_sample * 8 < format.bits_per_channel {
            return Err(format!(
                "container de {} bytes não comporta {} bits",
                format.bytes_per_sample, format.bits_per_channel
            ));
        }
        if format.sample_type == SampleType::Float && format.bits_per_channel != 32 {
            return Err(format!(
                "float de {} bits não é suportado; o device precisa publicar float32",
                format.bits_per_channel
            ));
        }
        Ok(Self {
            format,
            noise: Noise::new(seed),
            clipped: 0,
        })
    }

    /// Amostras que bateram no fundo de escala. Diferente do contador do processador: aqui a
    /// ceifa é do arredondamento, não do filtro.
    pub fn clipped(&self) -> u64 {
        self.clipped
    }

    pub fn bytes_per_sample(&self) -> usize {
        self.format.bytes_per_sample as usize
    }

    /// Escreve `samples` em `dst` no formato do device. Devolve quantos bytes escreveu.
    ///
    /// # Errors
    ///
    /// Se `dst` não comportar todas as amostras.
    pub fn write(&mut self, samples: &[f32], dst: &mut [u8]) -> Result<usize, String> {
        let width = self.format.bytes_per_sample as usize;
        let needed = samples.len() * width;
        if dst.len() < needed {
            return Err(format!(
                "buffer de {} bytes não comporta {} amostras de {width} bytes",
                dst.len(),
                samples.len()
            ));
        }

        match self.format.sample_type {
            SampleType::Float => self.write_float(samples, dst, width),
            SampleType::Integer => self.write_integer(samples, dst, width),
        }
        Ok(needed)
    }

    fn write_float(&mut self, samples: &[f32], dst: &mut [u8], width: usize) {
        for (i, &sample) in samples.iter().enumerate() {
            let bytes = sample.to_le_bytes();
            dst[i * width..i * width + 4].copy_from_slice(&bytes);
        }
    }

    fn write_integer(&mut self, samples: &[f32], dst: &mut [u8], width: usize) {
        let bits = self.format.bits_per_channel;
        let scale = (1u64 << (bits - 1)) as f64;
        let max = scale - 1.0;
        let min = -scale;
        let sample_bytes = (bits / 8) as usize;

        for (i, &sample) in samples.iter().enumerate() {
            let scaled = sample as f64 * scale + self.noise.tpdf();
            let rounded = scaled.round();
            let limited = rounded.clamp(min, max);
            if limited != rounded {
                self.clipped += 1;
            }

            // Complemento de dois na largura da amostra, little-endian. Os bytes do container
            // que sobram ficam nos mais significativos e recebem zero: é o alinhamento baixo,
            // que é o que o Core Audio publica quando não marca `kAudioFormatFlagIsAlignedHigh`.
            let value = limited as i64;
            let slot = &mut dst[i * width..(i + 1) * width];
            slot.fill(0);
            for (b, byte) in slot.iter_mut().take(sample_bytes).enumerate() {
                *byte = ((value >> (8 * b)) & 0xFF) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formato(sample_type: SampleType, bits: u32, bytes: u32) -> OutputFormat {
        OutputFormat {
            sample_type,
            bits_per_channel: bits,
            bytes_per_sample: bytes,
        }
    }

    /// Lê de volta um inteiro com sinal, na largura e no alinhamento em que foi escrito.
    fn ler_inteiro(bytes: &[u8], bits: u32) -> i64 {
        let width = (bits / 8) as usize;
        let mut value: i64 = 0;
        for b in (0..width).rev() {
            value = (value << 8) | bytes[b] as i64;
        }
        let sign_bit = 1i64 << (bits - 1);
        if value & sign_bit != 0 {
            value -= 1i64 << bits;
        }
        value
    }

    #[test]
    fn float32_atravessa_sem_alteracao() {
        let mut q = Quantizer::new(formato(SampleType::Float, 32, 4), 1).expect("quantizador");
        let samples = [0.0f32, 0.5, -0.5, 0.999_999];
        let mut dst = vec![0u8; 16];

        let escritos = q.write(&samples, &mut dst).expect("deveria escrever");

        assert_eq!(escritos, 16);
        for (i, &esperado) in samples.iter().enumerate() {
            let bytes: [u8; 4] = dst[i * 4..i * 4 + 4].try_into().unwrap();
            assert_eq!(f32::from_le_bytes(bytes), esperado);
        }
    }

    // Sem dither o valor seria exato; com ele fica a um LSB de distância. É esse o preço, e é
    // ele que este teste fixa.
    #[test]
    fn int16_fica_a_um_lsb_do_valor_exato() {
        let mut q = Quantizer::new(formato(SampleType::Integer, 16, 2), 7).expect("quantizador");
        let samples = [0.5f32; 64];
        let mut dst = vec![0u8; 128];

        q.write(&samples, &mut dst).expect("deveria escrever");

        for i in 0..64 {
            let value = ler_inteiro(&dst[i * 2..], 16);
            assert!(
                (value - 16384).abs() <= 1,
                "amostra {i}: {value} longe de 16384"
            );
        }
    }

    /// O dither precisa mesmo variar. Um gerador quebrado que devolvesse sempre zero passaria
    /// no teste de proximidade acima e falharia aqui — que é o ponto.
    #[test]
    fn o_dither_espalha_o_valor_quantizado() {
        let mut q = Quantizer::new(formato(SampleType::Integer, 16, 2), 7).expect("quantizador");
        let samples = [0.5f32; 512];
        let mut dst = vec![0u8; 1024];

        q.write(&samples, &mut dst).expect("deveria escrever");

        let mut distintos = std::collections::BTreeSet::new();
        for i in 0..512 {
            distintos.insert(ler_inteiro(&dst[i * 2..], 16));
        }

        assert!(
            distintos.len() >= 2,
            "todas as amostras caíram no mesmo valor: {distintos:?}"
        );
    }

    /// O erro médio do dither TPDF tende a zero: é isso que faz um sinal abaixo do LSB
    /// sobreviver à quantização em vez de sumir.
    #[test]
    fn o_erro_medio_do_dither_tende_a_zero() {
        let mut q = Quantizer::new(formato(SampleType::Integer, 16, 2), 11).expect("quantizador");
        // Entre dois inteiros de propósito: em cima de um deles, arredondar sem dither
        // também daria viés zero, e o teste não teria o que reprovar.
        let alvo = 0.2501f32;
        let samples = [alvo; 4096];
        let mut dst = vec![0u8; 8192];

        q.write(&samples, &mut dst).expect("deveria escrever");

        let exato = alvo as f64 * 32768.0;
        let mut soma = 0.0;
        for i in 0..4096 {
            soma += ler_inteiro(&dst[i * 2..], 16) as f64 - exato;
        }

        assert!((soma / 4096.0).abs() < 0.05, "viés de {}", soma / 4096.0);
    }

    #[test]
    fn int24_empacotado_em_tres_bytes() {
        let mut q = Quantizer::new(formato(SampleType::Integer, 24, 3), 3).expect("quantizador");
        let mut dst = vec![0u8; 6];

        q.write(&[0.5f32, -0.5], &mut dst)
            .expect("deveria escrever");

        assert!((ler_inteiro(&dst[0..], 24) - 4_194_304).abs() <= 1);
        assert!((ler_inteiro(&dst[3..], 24) + 4_194_304).abs() <= 1);
    }

    /// int24 em container de 32 bits: os bits vão nos bytes baixos e o byte que sobra fica
    /// zerado. Escrever no lugar errado desloca a amostra em 8 bits — 48 dB de erro.
    #[test]
    fn int24_em_container_de_quatro_bytes_alinha_embaixo() {
        let mut q = Quantizer::new(formato(SampleType::Integer, 24, 4), 3).expect("quantizador");
        let mut dst = vec![0xAAu8; 8];

        q.write(&[0.5f32, 0.0], &mut dst).expect("deveria escrever");

        assert!((ler_inteiro(&dst[0..], 24) - 4_194_304).abs() <= 1);
        assert_eq!(dst[3], 0, "byte de padding sujo");
        assert_eq!(dst[7], 0, "byte de padding sujo");
    }

    #[test]
    fn valor_fora_da_escala_e_ceifado_e_contado() {
        let mut q = Quantizer::new(formato(SampleType::Integer, 16, 2), 5).expect("quantizador");
        let mut dst = vec![0u8; 8];

        q.write(&[2.0f32, -2.0, 1.0, -1.0], &mut dst)
            .expect("deveria escrever");

        assert_eq!(ler_inteiro(&dst[0..], 16), 32767);
        assert_eq!(ler_inteiro(&dst[2..], 16), -32768);
        assert!(q.clipped() >= 2, "ceifas contadas: {}", q.clipped());
    }

    #[test]
    fn buffer_pequeno_demais_e_recusado() {
        let mut q = Quantizer::new(formato(SampleType::Integer, 16, 2), 1).expect("quantizador");

        assert!(q.write(&[0.0f32; 8], &mut [0u8; 8]).is_err());
    }

    #[test]
    fn formatos_impossiveis_sao_recusados() {
        assert!(Quantizer::new(formato(SampleType::Integer, 20, 4), 1).is_err()); // fora do byte
        assert!(Quantizer::new(formato(SampleType::Integer, 32, 2), 1).is_err()); // não cabe
        assert!(Quantizer::new(formato(SampleType::Float, 64, 8), 1).is_err()); // float64
        assert!(Quantizer::new(formato(SampleType::Integer, 0, 4), 1).is_err());
    }
}
