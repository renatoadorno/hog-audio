//! Regras puras de perda do device: o que o HAL diz agora, comparado com o que o device era
//! no fim da aquisição. Sem hardware, para cobrir cada caso em teste.

use coreaudio_sys::AudioStreamBasicDescription;

/// O formato físico reduzido ao que importa para o bit-perfect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicalFormat {
    pub sample_rate: f64,
    pub format_id: u32,
    pub flags: u32,
    pub bits_per_channel: u32,
    pub channels: u32,
    pub bytes_per_frame: u32,
}

/// A tolerância de sempre para comparar rates: o HAL devolve `f64` com ruído.
fn same_rate(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.5
}

impl PhysicalFormat {
    pub(crate) fn same_as(&self, other: &PhysicalFormat) -> bool {
        same_rate(self.sample_rate, other.sample_rate)
            && self.format_id == other.format_id
            && self.flags == other.flags
            && self.bits_per_channel == other.bits_per_channel
            && self.channels == other.channels
            && self.bytes_per_frame == other.bytes_per_frame
    }

    pub fn from_asbd(format: &AudioStreamBasicDescription) -> Self {
        Self {
            sample_rate: format.mSampleRate,
            format_id: format.mFormatID,
            flags: format.mFormatFlags,
            bits_per_channel: format.mBitsPerChannel,
            channels: format.mChannelsPerFrame,
            bytes_per_frame: format.mBytesPerFrame,
        }
    }
}

/// O device no fim da aquisição, lido do HAL. O veredito compara com isto, nunca com o
/// formato pedido: o device pode ter ajustado o pedido, e isso não é problema.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Baseline {
    pub pid: i32,
    pub nominal_rate: f64,
    pub physical: PhysicalFormat,
}

/// O que o HAL diz agora. `None` é uma leitura que falhou.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LiveSignals {
    pub alive: Option<bool>,
    pub hog_owner: Option<i32>,
    pub nominal_rate: Option<f64>,
    pub physical: Option<PhysicalFormat>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Disconnected,
    /// `owner` é −1 quando ninguém tem o hog.
    HogLost {
        owner: i32,
    },
    FormatChanged,
}

pub fn verdict(live: &LiveSignals, baseline: &Baseline) -> Option<Problem> {
    if live.alive != Some(true) {
        return Some(Problem::Disconnected);
    }
    // Um device vivo que não responde nem sobre o hog não serve para tocar.
    let owner = match live.hog_owner {
        Some(owner) => owner,
        None => return Some(Problem::Disconnected),
    };
    if owner != baseline.pid {
        return Some(Problem::HogLost { owner });
    }
    let rate_ok = live
        .nominal_rate
        .is_some_and(|rate| same_rate(rate, baseline.nominal_rate));
    let format_ok = live
        .physical
        .is_some_and(|format| format.same_as(&baseline.physical));
    if !rate_ok || !format_ok {
        return Some(Problem::FormatChanged);
    }
    None
}

/// A mensagem para quem usa o player. `app` é o nome do processo dono do hog, quando há um.
pub fn describe(problem: Problem, app: Option<&str>) -> String {
    match problem {
        Problem::Disconnected => {
            "o device de saída foi desconectado; a reprodução parou".to_string()
        }
        Problem::HogLost { owner } if owner < 0 => {
            "o device perdeu o modo exclusivo; a reprodução parou".to_string()
        }
        Problem::HogLost { owner } => format!(
            "o device foi tomado em modo exclusivo por {} (pid {owner}); a reprodução parou",
            app.unwrap_or("outro processo")
        ),
        Problem::FormatChanged => "o formato do device foi alterado por fora; a reprodução \
                                   parou para não tocar fora do bit-perfect"
            .to_string(),
    }
}

/// O volume pedido vale só para o device em que foi pedido: 80% num fone pode ser alto demais
/// numa caixa. Um pedido sem device (o `--volume` da linha de comando) vale para qualquer um.
pub fn request_applies(request_uid: Option<&str>, device_uid: &str) -> bool {
    request_uid.is_none_or(|uid| uid == device_uid)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EU: i32 = 4242;

    fn formato(sample_rate: f64) -> PhysicalFormat {
        PhysicalFormat {
            sample_rate,
            format_id: coreaudio_sys::kAudioFormatLinearPCM,
            flags: coreaudio_sys::kAudioFormatFlagIsFloat,
            bits_per_channel: 32,
            channels: 2,
            bytes_per_frame: 8,
        }
    }

    fn base() -> Baseline {
        Baseline {
            pid: EU,
            nominal_rate: 96_000.0,
            physical: formato(96_000.0),
        }
    }

    fn tudo_certo() -> LiveSignals {
        LiveSignals {
            alive: Some(true),
            hog_owner: Some(EU),
            nominal_rate: Some(96_000.0),
            physical: Some(formato(96_000.0)),
        }
    }

    #[test]
    fn device_igual_ao_da_aquisicao_nao_e_problema() {
        assert_eq!(verdict(&tudo_certo(), &base()), None);
    }

    #[test]
    fn device_que_morreu_ou_nao_responde_foi_desconectado() {
        let morto = LiveSignals {
            alive: Some(false),
            ..tudo_certo()
        };
        let mudo = LiveSignals {
            alive: None,
            ..tudo_certo()
        };
        let sem_hog_legivel = LiveSignals {
            hog_owner: None,
            ..tudo_certo()
        };
        assert_eq!(verdict(&morto, &base()), Some(Problem::Disconnected));
        assert_eq!(verdict(&mudo, &base()), Some(Problem::Disconnected));
        assert_eq!(
            verdict(&sem_hog_legivel, &base()),
            Some(Problem::Disconnected)
        );
    }

    #[test]
    fn desconectado_vence_os_outros_sinais() {
        let tudo_errado = LiveSignals {
            alive: Some(false),
            hog_owner: Some(99),
            nominal_rate: Some(44_100.0),
            physical: Some(formato(44_100.0)),
        };
        assert_eq!(verdict(&tudo_errado, &base()), Some(Problem::Disconnected));
    }

    #[test]
    fn hog_de_outro_processo_ou_de_ninguem_foi_perdido() {
        let outro = LiveSignals {
            hog_owner: Some(99),
            ..tudo_certo()
        };
        let ninguem = LiveSignals {
            hog_owner: Some(-1),
            ..tudo_certo()
        };
        assert_eq!(
            verdict(&outro, &base()),
            Some(Problem::HogLost { owner: 99 })
        );
        assert_eq!(
            verdict(&ninguem, &base()),
            Some(Problem::HogLost { owner: -1 })
        );
    }

    #[test]
    fn rate_ou_formato_diferente_da_aquisicao_mudou_por_fora() {
        let rate = LiveSignals {
            nominal_rate: Some(44_100.0),
            ..tudo_certo()
        };
        let mut bits = formato(96_000.0);
        bits.bits_per_channel = 24;
        let formato_fisico = LiveSignals {
            physical: Some(bits),
            ..tudo_certo()
        };
        let ilegivel = LiveSignals {
            physical: None,
            ..tudo_certo()
        };
        assert_eq!(verdict(&rate, &base()), Some(Problem::FormatChanged));
        assert_eq!(
            verdict(&formato_fisico, &base()),
            Some(Problem::FormatChanged)
        );
        assert_eq!(verdict(&ilegivel, &base()), Some(Problem::FormatChanged));
    }

    #[test]
    fn rate_com_ruido_de_ponto_flutuante_continua_igual() {
        let quase = LiveSignals {
            nominal_rate: Some(96_000.000_1),
            physical: Some(formato(96_000.000_1)),
            ..tudo_certo()
        };
        assert_eq!(verdict(&quase, &base()), None);
    }

    #[test]
    fn cada_problema_tem_mensagem_propria() {
        let desconectado = describe(Problem::Disconnected, None);
        let tomado = describe(Problem::HogLost { owner: 99 }, Some("Spotifast"));
        let tomado_sem_nome = describe(Problem::HogLost { owner: 99 }, None);
        let solto = describe(Problem::HogLost { owner: -1 }, None);
        let formato = describe(Problem::FormatChanged, None);

        assert!(desconectado.contains("desconectado"), "{desconectado}");
        assert!(
            tomado.contains("Spotifast") && tomado.contains("99"),
            "{tomado}"
        );
        assert!(tomado_sem_nome.contains("99"), "{tomado_sem_nome}");
        assert!(solto.contains("exclusivo"), "{solto}");
        assert!(formato.contains("formato"), "{formato}");
        let todas = [desconectado, tomado, solto, formato];
        for (i, a) in todas.iter().enumerate() {
            for b in &todas[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn volume_pedido_vale_so_no_device_em_que_foi_pedido() {
        assert!(request_applies(Some("fone"), "fone"));
        assert!(!request_applies(Some("fone"), "caixas"));
        assert!(request_applies(None, "caixas"));
    }
}
