//! A curva de afinação: pares de frequência e ganho que descrevem a resposta que o filtro
//! deve ter. Puro, sem Core Audio e sem FFT — ler e interpolar a curva não depende de como
//! ela vira filtro, e essa separação é o que permite conferir a curva sem tocar no device.

/// Um ponto medido ou desenhado da curva.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvePoint {
    pub hz: f64,
    pub db: f64,
}

/// Ganhos acima disto não descrevem afinação: descrevem erro de leitura ou uma curva em
/// escala errada. Recusar é melhor do que mandar +60 dB para o DAC.
const MAX_ABS_DB: f64 = 40.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    points: Vec<CurvePoint>,
}

impl Curve {
    /// Exige pontos em frequência estritamente crescente: uma curva com a mesma frequência
    /// duas vezes não define um ganho, e uma fora de ordem quase sempre é arquivo corrompido.
    pub fn from_points(points: Vec<CurvePoint>) -> Result<Self, String> {
        if points.len() < 2 {
            return Err("a curva precisa de ao menos dois pontos".to_string());
        }
        for (i, p) in points.iter().enumerate() {
            if !p.hz.is_finite() || p.hz <= 0.0 {
                return Err(format!("frequência inválida no ponto {}: {}", i + 1, p.hz));
            }
            if !p.db.is_finite() || p.db.abs() > MAX_ABS_DB {
                return Err(format!(
                    "ganho de {} dB no ponto {} está fora de ±{MAX_ABS_DB} dB",
                    p.db,
                    i + 1
                ));
            }
            if i > 0 && p.hz <= points[i - 1].hz {
                return Err(format!(
                    "frequências fora de ordem: {} Hz vem depois de {} Hz",
                    p.hz,
                    points[i - 1].hz
                ));
            }
        }
        Ok(Self { points })
    }

    pub fn points(&self) -> &[CurvePoint] {
        &self.points
    }

    /// Ganho em `hz`, interpolado **em log-frequência**: é assim que a curva é desenhada e
    /// lida, e interpolar em escala linear entorta o grave, onde os pontos são mais espaçados
    /// em Hz e mais próximos em oitavas.
    ///
    /// Fora dos extremos o ganho é constante, igual ao ponto da ponta. Extrapolar a
    /// inclinação levaria a curva a valores que ninguém mediu — a queda final desta curva,
    /// prolongada, chegaria a dezenas de dB negativos logo acima de 20 kHz.
    pub fn gain_at(&self, hz: f64) -> f64 {
        let first = self.points[0];
        let last = self.points[self.points.len() - 1];
        if !hz.is_finite() || hz <= first.hz {
            return first.db;
        }
        if hz >= last.hz {
            return last.db;
        }

        for pair in self.points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if hz <= b.hz {
                let t = (hz.ln() - a.hz.ln()) / (b.hz.ln() - a.hz.ln());
                return a.db + t * (b.db - a.db);
            }
        }
        last.db
    }

    /// O maior ganho da curva. Como a interpolação é linear entre os pontos e constante fora
    /// deles, o máximo da curva contínua está sempre em um dos pontos.
    pub fn peak_db(&self) -> f64 {
        let mut peak = f64::NEG_INFINITY;
        for p in &self.points {
            if p.db > peak {
                peak = p.db;
            }
        }
        peak
    }

    /// Atenuação a aplicar antes do filtro para que nada nele tenha ganho positivo. Ganho
    /// digital acima de 0 dBFS não fica alto: satura, e saturação em pico de grave é o tipo de
    /// distorção que se confunde com "o filtro está funcionando".
    ///
    /// Curva que já só atenua não recebe preamp positivo: subir o sinal por conta própria não
    /// foi o que se pediu.
    pub fn preamp_db(&self) -> f64 {
        let peak = self.peak_db();
        if peak > 0.0 { -peak } else { 0.0 }
    }

    /// A curva deslocada pelo preamp — o que o filtro de fato realiza. O timbre é o mesmo: o
    /// que define o som é a forma da curva, não o quanto ela inteira está alta.
    pub fn normalized(&self) -> Self {
        let preamp = self.preamp_db();
        let points = self
            .points
            .iter()
            .map(|p| CurvePoint {
                hz: p.hz,
                db: p.db + preamp,
            })
            .collect();
        Self { points }
    }
}

/// Lê o formato de texto: uma frequência e um ganho por linha, separados por espaço, vírgula
/// ou ponto e vírgula. `#` comenta até o fim da linha.
///
/// Uma primeira linha não numérica é aceita como cabeçalho e ignorada, porque é assim que os
/// CSVs exportados de medição chegam ("frequency,raw"). Depois dela, linha que não parseia é
/// erro: um arquivo meio lido produziria uma curva plausível e errada.
pub fn parse(text: &str) -> Result<Curve, String> {
    let mut points = Vec::new();
    let mut seen_data = false;

    for (index, raw_line) in text.lines().enumerate() {
        let line = match raw_line.split('#').next() {
            Some(before_comment) => before_comment.trim(),
            None => "",
        };
        if line.is_empty() {
            continue;
        }

        let fields: Vec<&str> = line
            .split([' ', '\t', ',', ';'])
            .filter(|f| !f.is_empty())
            .collect();
        let parsed = match fields.as_slice() {
            [hz, db, ..] => match (hz.parse::<f64>(), db.parse::<f64>()) {
                (Ok(hz), Ok(db)) => Some(CurvePoint { hz, db }),
                _ => None,
            },
            _ => None,
        };

        let Some(point) = parsed else {
            if !seen_data {
                continue; // cabeçalho do CSV
            }
            return Err(format!(
                "linha {}: não entendi \"{}\"; esperava frequência e ganho",
                index + 1,
                line
            ));
        };

        seen_data = true;
        points.push(point);
    }

    if points.is_empty() {
        return Err("a curva não tem nenhum ponto".to_string());
    }
    Curve::from_points(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn curve(points: &[(f64, f64)]) -> Curve {
        let points = points
            .iter()
            .map(|&(hz, db)| CurvePoint { hz, db })
            .collect();
        Curve::from_points(points).expect("curva do teste deveria ser válida")
    }

    #[test]
    fn parse_aceita_espaco_virgula_e_comentario() {
        let c =
            parse("# comentário\n20 12.5\n\n100,7.6\n1000;-0.6  # fim\n").expect("deveria parsear");

        assert_eq!(c.points().len(), 3);
        assert!(near(c.points()[0].hz, 20.0));
        assert!(near(c.points()[2].db, -0.6));
    }

    #[test]
    fn parse_ignora_cabecalho_de_csv() {
        let c = parse("frequency,raw\n20,12.5\n20000,-10.6\n").expect("deveria parsear");

        assert_eq!(c.points().len(), 2);
    }

    // Uma linha corrompida no meio produziria uma curva plausível e errada — pior que recusar.
    #[test]
    fn parse_recusa_lixo_depois_dos_dados() {
        let erro = parse("20 12.5\n100 7.6\nlixo aqui\n1000 -0.6\n").expect_err("deveria recusar");

        assert!(erro.contains("linha 3"), "mensagem sem a linha: {erro}");
    }

    #[test]
    fn parse_recusa_arquivo_sem_pontos() {
        assert!(parse("# só comentário\n\n").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn curva_precisa_de_dois_pontos() {
        assert!(
            Curve::from_points(vec![CurvePoint {
                hz: 1000.0,
                db: 0.0
            }])
            .is_err()
        );
    }

    #[test]
    fn frequencias_fora_de_ordem_sao_recusadas() {
        let pontos = vec![
            CurvePoint {
                hz: 1000.0,
                db: 0.0,
            },
            CurvePoint { hz: 100.0, db: 3.0 },
        ];

        assert!(Curve::from_points(pontos).is_err());
    }

    #[test]
    fn frequencia_repetida_e_recusada() {
        let pontos = vec![
            CurvePoint { hz: 100.0, db: 0.0 },
            CurvePoint { hz: 100.0, db: 3.0 },
        ];

        assert!(Curve::from_points(pontos).is_err());
    }

    #[test]
    fn frequencia_nao_positiva_e_recusada() {
        let pontos = vec![
            CurvePoint { hz: 0.0, db: 0.0 },
            CurvePoint { hz: 100.0, db: 3.0 },
        ];

        assert!(Curve::from_points(pontos).is_err());
    }

    #[test]
    fn ganho_absurdo_e_recusado() {
        let pontos = vec![
            CurvePoint { hz: 20.0, db: 60.0 },
            CurvePoint {
                hz: 20000.0,
                db: 0.0,
            },
        ];

        assert!(Curve::from_points(pontos).is_err());
    }

    #[test]
    fn ganho_nos_proprios_pontos_e_exato() {
        let c = curve(&[(100.0, 6.0), (1000.0, 0.0), (10000.0, -6.0)]);

        assert!(near(c.gain_at(100.0), 6.0));
        assert!(near(c.gain_at(1000.0), 0.0));
        assert!(near(c.gain_at(10000.0), -6.0));
    }

    // A média geométrica de 100 e 1000 é ~316 Hz: em log-frequência ela cai na metade do
    // segmento, e o ganho tem de ser a média dos dois. Interpolação linear em Hz devolveria
    // outra coisa, e é justamente esse o erro que este teste existe para pegar.
    #[test]
    fn interpolacao_e_em_log_frequencia() {
        let c = curve(&[(100.0, 6.0), (1000.0, 0.0)]);

        let meio_geometrico = (100.0f64 * 1000.0).sqrt();

        assert!(near(c.gain_at(meio_geometrico), 3.0));
        // O meio aritmético (550 Hz) fica bem além da metade em log: o ganho ali é menor que 3.
        assert!(c.gain_at(550.0) < 3.0);
    }

    #[test]
    fn fora_dos_extremos_o_ganho_e_constante() {
        let c = curve(&[(20.0, 12.0), (20000.0, -10.0)]);

        assert!(near(c.gain_at(5.0), 12.0));
        assert!(near(c.gain_at(0.0), 12.0));
        assert!(near(c.gain_at(48000.0), -10.0));
    }

    #[test]
    fn preamp_derruba_o_pico_para_zero() {
        let c = curve(&[(20.0, 12.5), (1000.0, -0.6), (3200.0, 10.0)]);

        assert!(near(c.peak_db(), 12.5));
        assert!(near(c.preamp_db(), -12.5));

        let n = c.normalized();

        assert!(near(n.peak_db(), 0.0));
        assert!(near(n.gain_at(1000.0), -13.1));
    }

    // A forma é o que define o timbre: normalizar não pode mexer na diferença entre bandas.
    #[test]
    fn normalizar_preserva_a_forma_da_curva() {
        let c = curve(&[(20.0, 12.5), (1000.0, -0.6), (3200.0, 10.0)]);
        let n = c.normalized();

        let antes = c.gain_at(20.0) - c.gain_at(1000.0);
        let depois = n.gain_at(20.0) - n.gain_at(1000.0);

        assert!(near(antes, depois));
    }

    #[test]
    fn curva_que_so_atenua_nao_ganha_preamp() {
        let c = curve(&[(20.0, -2.0), (20000.0, -12.0)]);

        assert!(near(c.preamp_db(), 0.0));
        assert_eq!(c.normalized(), c);
    }
}

/// A curva versionada no repositório. Ler o arquivo real, e não uma cópia embutida, é o que
/// faz um erro de digitação nele reprovar aqui em vez de aparecer no fone.
#[cfg(test)]
mod curva_versionada {
    use super::*;

    fn dourada() -> Curve {
        let path = "../curves/dourada.curve";
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("não consegui ler {path}: {e}"));
        parse(&text).expect("a curva versionada deveria ser válida")
    }

    #[test]
    fn dourada_cobre_a_banda_audivel_inteira() {
        let c = dourada();
        let pontos = c.points();

        assert!(
            pontos.len() >= 40,
            "curva rala demais: {} pontos",
            pontos.len()
        );
        assert!(pontos[0].hz <= 20.0);
        assert!(pontos[pontos.len() - 1].hz >= 20000.0);
    }

    /// O que o filtro vai realizar de fato. O pico vai a 0 dB e o resto desce junto: é a forma
    /// que define o timbre, não a altura absoluta.
    #[test]
    fn dourada_normalizada_so_atenua() {
        let n = dourada().normalized();

        for p in n.points() {
            assert!(p.db <= 1e-9, "ganho positivo em {} Hz: {} dB", p.hz, p.db);
        }
        assert!((n.gain_at(20.0) - -0.1).abs() < 0.05); // grave, no topo da curva
        assert!((n.gain_at(1000.0) - -13.2).abs() < 0.05); // referência
        assert!((n.gain_at(3200.0) - -2.6).abs() < 0.05); // pico de presença
        assert!((n.gain_at(20000.0) - -23.2).abs() < 0.05); // extremo agudo
    }

    /// O relevo que se ouve: o grave sobe ~13 dB sobre 1 kHz, e a presença de 3 kHz, ~10,6 dB.
    #[test]
    fn dourada_mantem_o_relevo_depois_do_preamp() {
        let c = dourada();
        let n = c.normalized();

        assert!((c.preamp_db() - -12.6).abs() < 0.05);
        assert!(((n.gain_at(20.0) - n.gain_at(1000.0)) - 13.1).abs() < 0.05);
        assert!(((n.gain_at(3200.0) - n.gain_at(1000.0)) - 10.6).abs() < 0.05);
    }
}
