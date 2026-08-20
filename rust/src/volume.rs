//! Interpretação do volume pedido e regra do teto de segurança. Puro, sem Core Audio: a
//! conversão entre porcentagem e decibéis depende da curva do amplificador e fica na camada
//! que fala com o hardware, mas decidir o que foi pedido não precisa de device nenhum.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VolumeUnit {
    Percent,
    Decibels,
}

#[derive(Clone, Debug)]
pub struct VolumeRequest {
    pub valid: bool,
    pub unit: VolumeUnit,
    pub value: f64, // 0..1 quando Percent; decibéis (≤ 0) quando Decibels
    pub reason: String,
}

impl VolumeRequest {
    fn invalid(unit: VolumeUnit, reason: String) -> Self {
        Self {
            valid: false,
            unit,
            value: 0.0,
            reason,
        }
    }

    fn ok(unit: VolumeUnit, value: f64) -> Self {
        Self {
            valid: true,
            unit,
            value,
            reason: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CeilingDecision {
    pub apply: bool,
    pub scalar: f64,
}

/// Converte sem aceitar sobras: "35x" é recusado, ao contrário do que um parse permissivo
/// faria ao devolver 35 e ignorar o resto.
fn parse_number(text: &str) -> Option<f64> {
    if text.is_empty() {
        return None;
    }
    let mut chars = text.chars().peekable();
    let mut sign = 1.0;
    if let Some(&c) = chars.peek()
        && (c == '+' || c == '-')
    {
        if c == '-' {
            sign = -1.0;
        }
        chars.next();
    }

    let rest: String = chars.collect();
    if rest.is_empty() {
        return None;
    }
    if !rest.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    if rest.matches('.').count() > 1 {
        return None;
    }
    if !rest.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }

    rest.parse::<f64>().ok().map(|v| sign * v)
}

/// Aceita "35", "35%", "12.5", "-18dB", "-18 db". Recusa faixas impossíveis em vez de as
/// truncar em silêncio: um volume interpretado errado chega ao fone como volume errado.
pub fn parse_volume(text: &str) -> VolumeRequest {
    let input = text.trim();
    if input.is_empty() {
        return VolumeRequest::invalid(VolumeUnit::Percent, "volume vazio".to_string());
    }

    let lower = input.to_ascii_lowercase();
    if lower.len() >= 3 && lower.ends_with("db") {
        let number_part = input[..input.len() - 2].trim();
        let Some(decibels) = parse_number(number_part) else {
            return VolumeRequest::invalid(
                VolumeUnit::Decibels,
                format!("não entendi os decibéis em \"{text}\""),
            );
        };
        if decibels > 0.0 {
            return VolumeRequest::invalid(
                VolumeUnit::Decibels,
                "o amplificador atenua a partir de 0 dB; não há ganho acima disso".to_string(),
            );
        }
        return VolumeRequest::ok(VolumeUnit::Decibels, decibels);
    }

    let percent_part = input.strip_suffix('%').unwrap_or(input).trim();
    let Some(percent) = parse_number(percent_part) else {
        return VolumeRequest::invalid(
            VolumeUnit::Percent,
            format!("não entendi \"{text}\"; use algo como 35, 35% ou -18dB"),
        );
    };
    if !(0.0..=100.0).contains(&percent) {
        return VolumeRequest::invalid(
            VolumeUnit::Percent,
            "porcentagem fora de 0 a 100".to_string(),
        );
    }
    VolumeRequest::ok(VolumeUnit::Percent, percent / 100.0)
}

/// Sem pedido explícito, o volume só é tocado quando passa do teto — para que esquecer a
/// flag não signifique receber o volume cheio no fone.
pub fn apply_ceiling(current_scalar: f64, ceiling_scalar: f64) -> CeilingDecision {
    if current_scalar <= ceiling_scalar {
        return CeilingDecision::default();
    }
    CeilingDecision {
        apply: true,
        scalar: ceiling_scalar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn porcentagem_simples() {
        let v = parse_volume("35");

        assert!(v.valid);
        assert_eq!(v.unit, VolumeUnit::Percent);
        assert!(near(v.value, 0.35));
    }

    #[test]
    fn porcentagem_com_sinal_de_percentual() {
        let v = parse_volume("35%");

        assert!(v.valid);
        assert_eq!(v.unit, VolumeUnit::Percent);
        assert!(near(v.value, 0.35));
    }

    #[test]
    fn extremos_de_porcentagem() {
        assert!(parse_volume("0").valid);
        assert!(parse_volume("100").valid);
        assert!(near(parse_volume("0").value, 0.0));
        assert!(near(parse_volume("100").value, 1.0));
    }

    #[test]
    fn porcentagem_fracionaria() {
        let v = parse_volume("12.5");

        assert!(v.valid);
        assert!(near(v.value, 0.125));
    }

    #[test]
    fn decibeis_em_qualquer_caixa() {
        for texto in ["-18dB", "-18db", "-18DB", "-18 dB"] {
            let v = parse_volume(texto);
            assert!(v.valid, "{texto} deveria ser válido");
            assert_eq!(v.unit, VolumeUnit::Decibels);
            assert!(near(v.value, -18.0));
        }
    }

    #[test]
    fn zero_decibeis_e_valido() {
        let v = parse_volume("0dB");

        assert!(v.valid);
        assert_eq!(v.unit, VolumeUnit::Decibels);
        assert!(near(v.value, 0.0));
    }

    // O amp atenua a partir de 0 dB; pedir ganho acima disso não existe no hardware.
    #[test]
    fn decibeis_positivos_sao_recusados() {
        let v = parse_volume("6dB");

        assert!(!v.valid);
        assert!(!v.reason.is_empty());
    }

    #[test]
    fn porcentagem_fora_da_faixa_e_recusada() {
        assert!(!parse_volume("101").valid);
        assert!(!parse_volume("-5").valid);
    }

    #[test]
    fn texto_sem_sentido_e_recusado() {
        for texto in ["", "abc", "35x", "dB", "--"] {
            assert!(!parse_volume(texto).valid, "{texto:?} deveria ser recusado");
        }
    }

    #[test]
    fn volume_acima_do_teto_e_baixado() {
        let d = apply_ceiling(1.0, 0.5);

        assert!(d.apply);
        assert!(near(d.scalar, 0.5));
    }

    #[test]
    fn volume_abaixo_do_teto_fica_como_esta() {
        assert!(!apply_ceiling(0.3, 0.5).apply);
    }

    #[test]
    fn volume_exatamente_no_teto_nao_e_mexido() {
        assert!(!apply_ceiling(0.5, 0.5).apply);
    }
}
