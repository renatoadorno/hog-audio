//! FFT radix-2 in-place, em Rust puro.
//!
//! Existe em vez de uma dependência porque o núcleo da afinação é testável sem Core Audio, e
//! trazer uma biblioteca só por isto custaria essa pureza sem devolver nada: a 8192 pontos
//! esta implementação leva ~100 µs, contra dezenas de milissegundos de áudio por bloco.
//!
//! O oráculo dos testes é a DFT ingênua, escrita direto da definição: ela é lenta demais para
//! reproduzir áudio e simples demais para errar, que é exatamente o que um oráculo precisa ser.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Complex {
    pub const ZERO: Self = Self { re: 0.0, im: 0.0 };

    pub fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    pub fn real(re: f64) -> Self {
        Self { re, im: 0.0 }
    }

    fn add(self, other: Self) -> Self {
        Self::new(self.re + other.re, self.im + other.im)
    }

    fn sub(self, other: Self) -> Self {
        Self::new(self.re - other.re, self.im - other.im)
    }

    fn mul(self, other: Self) -> Self {
        Self::new(
            self.re * other.re - self.im * other.im,
            self.re * other.im + self.im * other.re,
        )
    }

    pub fn magnitude(self) -> f64 {
        self.re.hypot(self.im)
    }

    /// `e^z`. O design do filtro precisa disto para sair do domínio do log de volta para a
    /// resposta complexa.
    pub fn exp(self) -> Self {
        let scale = self.re.exp();
        Self::new(scale * self.im.cos(), scale * self.im.sin())
    }
}

pub fn is_power_of_two(n: usize) -> bool {
    n > 0 && n & (n - 1) == 0
}

/// DFT direta, in-place.
///
/// # Panics
///
/// Se `data.len()` não for potência de dois. Radix-2 não tem o que fazer com outro tamanho, e
/// processar só um prefixo devolveria um espectro errado sem sinal nenhum de que errou.
pub fn forward(data: &mut [Complex]) {
    transform(data, -1.0);
}

/// DFT inversa, in-place, já normalizada por `1/N` — de modo que `inverse(forward(x)) == x`.
///
/// # Panics
///
/// Se `data.len()` não for potência de dois.
pub fn inverse(data: &mut [Complex]) {
    transform(data, 1.0);
    let scale = 1.0 / data.len() as f64;
    for value in data.iter_mut() {
        value.re *= scale;
        value.im *= scale;
    }
}

fn transform(data: &mut [Complex], sign: f64) {
    let n = data.len();
    assert!(
        is_power_of_two(n),
        "FFT radix-2 exige tamanho potência de dois; recebeu {n}"
    );
    if n == 1 {
        return;
    }

    // Permutação por reversão de bits: coloca cada amostra na posição em que as borboletas
    // seguintes a esperam, e permite fazer o resto sem buffer auxiliar.
    let mut target = 0usize;
    for source in 1..n {
        let mut bit = n >> 1;
        while target & bit != 0 {
            target ^= bit;
            bit >>= 1;
        }
        target |= bit;
        if source < target {
            data.swap(source, target);
        }
    }

    let mut len = 2;
    while len <= n {
        let angle = sign * 2.0 * PI / len as f64;
        let step = Complex::new(angle.cos(), angle.sin());
        for block in (0..n).step_by(len) {
            let mut twiddle = Complex::real(1.0);
            for offset in 0..len / 2 {
                let a = data[block + offset];
                let b = data[block + offset + len / 2].mul(twiddle);
                data[block + offset] = a.add(b);
                data[block + offset + len / 2] = a.sub(b);
                twiddle = twiddle.mul(step);
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A definição da DFT, sem otimização nenhuma: o oráculo contra o qual a radix-2 responde.
    fn dft_ingenua(data: &[Complex], sign: f64) -> Vec<Complex> {
        let n = data.len();
        let mut out = vec![Complex::ZERO; n];
        for (k, slot) in out.iter_mut().enumerate() {
            let mut sum = Complex::ZERO;
            for (t, value) in data.iter().enumerate() {
                let angle = sign * 2.0 * PI * (k * t) as f64 / n as f64;
                sum = sum.add(value.mul(Complex::new(angle.cos(), angle.sin())));
            }
            *slot = sum;
        }
        out
    }

    /// Gerador determinístico: teste que muda de entrada a cada rodada não é reproduzível.
    fn ruido(n: usize) -> Vec<Complex> {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let re = (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let im = (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
            out.push(Complex::new(re, im));
        }
        out
    }

    fn max_erro(a: &[Complex], b: &[Complex]) -> f64 {
        let mut worst: f64 = 0.0;
        for (x, y) in a.iter().zip(b.iter()) {
            worst = worst.max((x.re - y.re).abs()).max((x.im - y.im).abs());
        }
        worst
    }

    #[test]
    fn bate_com_a_dft_ingenua_em_varios_tamanhos() {
        for n in [2usize, 4, 8, 16, 64, 256] {
            let entrada = ruido(n);
            let esperado = dft_ingenua(&entrada, -1.0);

            let mut obtido = entrada.clone();
            forward(&mut obtido);

            assert!(
                max_erro(&obtido, &esperado) < 1e-12,
                "n={n}: erro de {}",
                max_erro(&obtido, &esperado)
            );
        }
    }

    #[test]
    fn inversa_bate_com_a_dft_ingenua() {
        let n = 64;
        let entrada = ruido(n);
        let mut esperado = dft_ingenua(&entrada, 1.0);
        for value in &mut esperado {
            value.re /= n as f64;
            value.im /= n as f64;
        }

        let mut obtido = entrada.clone();
        inverse(&mut obtido);

        assert!(max_erro(&obtido, &esperado) < 1e-12);
    }

    #[test]
    fn ida_e_volta_devolve_o_sinal() {
        let entrada = ruido(1024);

        let mut obtido = entrada.clone();
        forward(&mut obtido);
        inverse(&mut obtido);

        assert!(max_erro(&obtido, &entrada) < 1e-12);
    }

    // Um impulso na origem tem espectro plano e unitário. É o caso em que o resultado se sabe
    // de cabeça, e onde um erro de escala ou de sinal aparece na cara.
    #[test]
    fn impulso_tem_espectro_plano() {
        let mut data = vec![Complex::ZERO; 32];
        data[0] = Complex::real(1.0);

        forward(&mut data);

        for value in &data {
            assert!((value.magnitude() - 1.0).abs() < 1e-12);
        }
    }

    // Uma senoide exata em um bin concentra toda a energia nesse bin e no seu espelho.
    #[test]
    fn senoide_em_bin_exato_concentra_a_energia() {
        let n = 64;
        let bin = 5;
        let mut data = Vec::with_capacity(n);
        for t in 0..n {
            let angle = 2.0 * PI * (bin * t) as f64 / n as f64;
            data.push(Complex::real(angle.cos()));
        }

        forward(&mut data);

        for (k, value) in data.iter().enumerate() {
            let esperado = if k == bin || k == n - bin {
                n as f64 / 2.0
            } else {
                0.0
            };
            assert!(
                (value.magnitude() - esperado).abs() < 1e-9,
                "bin {k}: {} em vez de {esperado}",
                value.magnitude()
            );
        }
    }

    #[test]
    fn tamanho_de_um_e_identidade() {
        let mut data = vec![Complex::new(3.0, -2.0)];

        forward(&mut data);

        assert_eq!(data[0], Complex::new(3.0, -2.0));
    }

    #[test]
    #[should_panic(expected = "potência de dois")]
    fn tamanho_fora_da_potencia_de_dois_estoura() {
        let mut data = vec![Complex::ZERO; 24];

        forward(&mut data);
    }
}
