//! A casca imperativa da máquina de estados: aqui moram os efeitos sobre o hardware, o
//! arquivo e as threads. As regras de qual transição é permitida ficam em `transitions`,
//! puras e testáveis sem nada disso.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::device::{query_default_output_device, OutputDevice};
use crate::format::{self, Decision};
use crate::source::AudioSource;
use crate::status::SharedStatus;
use crate::transitions::{next_state, Command, PlayerState};

/// O que a interface precisa saber sobre a faixa recém-carregada.
#[derive(Debug)]
pub struct LoadedTrack {
    pub sample_rate: f64,
    pub bit_depth: u32,
    pub channels: u32,
    pub codec: String,
    pub device_name: String,
    pub total_seconds: f64,
}

pub struct Engine {
    status: Arc<SharedStatus>,
    inner: Mutex<EngineInner>,
}

struct EngineInner {
    state: PlayerState,
    path: Option<String>,
    source: Option<AudioSource>,
    device: Option<OutputDevice>,
    decision: Option<Decision>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            status: Arc::new(SharedStatus::new()),
            inner: Mutex::new(EngineInner {
                state: PlayerState::Idle,
                path: None,
                source: None,
                device: None,
                decision: None,
            }),
        }
    }

    /// Um pânico dentro de um comando não pode deixar o player inutilizável para sempre: o
    /// estado interno é recuperado em vez de propagar o envenenamento do mutex.
    fn lock(&self) -> MutexGuard<'_, EngineInner> {
        self.inner.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    pub fn status(&self) -> Arc<SharedStatus> {
        Arc::clone(&self.status)
    }

    pub fn state(&self) -> PlayerState {
        self.lock().state
    }

    pub fn load(&self, path: &str) -> Result<LoadedTrack, String> {
        // Abrir o arquivo primeiro, antes de descartar o que estava carregado: se o caminho
        // for inválido, o player continua exatamente como estava.
        let source = AudioSource::open(path)?;
        let file_format = source.format();
        let codec = source.codec_name().to_string();
        let total_frames = source.total_frames();

        // A negociação corre pelo mesmo motivo, e antes do lock: um arquivo válido cujo
        // formato o device recusa não pode apagar a faixa que já estava carregada e tocável.
        let device = query_default_output_device()?;
        let decision = format::negotiate(&file_format, &device.caps);
        if !decision.play {
            return Err(decision.reason);
        }

        let mut inner = self.lock();
        self.teardown(&mut inner);

        let device_name = device.name.clone();
        let target = next_state(inner.state, Command::Load).map_err(|e| e.message.to_string())?;

        inner.path = Some(path.to_string());
        inner.source = Some(source);
        inner.device = Some(device);
        inner.decision = Some(decision);
        inner.state = target;

        self.status.set_track(total_frames, file_format.sample_rate);
        self.status.set_state(target);

        Ok(LoadedTrack {
            sample_rate: file_format.sample_rate,
            bit_depth: file_format.bit_depth,
            channels: file_format.channels,
            codec,
            device_name,
            total_seconds: self.status.total_seconds(),
        })
    }

    /// Descarta o que estiver carregado. A Task 6 estende isto para também parar as threads e
    /// devolver o device — daí o `&self` já presente, embora esta versão não o use.
    fn teardown(&self, inner: &mut EngineInner) {
        inner.source = None;
        inner.decision = None;
        inner.device = None;
        inner.path = None;
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_novo_comeca_em_idle() {
        let engine = Engine::new();
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    fn arquivo_inexistente_falha_sem_mudar_de_estado() {
        // O arquivo é aberto antes de qualquer consulta ao device: um caminho inválido não
        // pode chegar perto do hardware.
        let engine = Engine::new();
        let erro = engine.load("/tmp/nao-existe-mesmo-12345.flac").unwrap_err();
        assert!(!erro.is_empty());
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    #[ignore = "precisa de um device de saída real"]
    fn load_de_flac_valido_vai_para_loaded() {
        let path = "../testdata/t96_24.flac";
        if !std::path::Path::new(path).exists() {
            eprintln!("pulando: {path} não existe (gere com ffmpeg)");
            return;
        }
        let engine = Engine::new();
        let track = engine.load(path).expect("deveria carregar");

        assert_eq!(engine.state(), PlayerState::Loaded);
        assert_eq!(track.sample_rate, 96000.0);
        assert_eq!(track.bit_depth, 24);
        assert_eq!(track.channels, 2);
        assert_eq!(track.codec, "flac");
        assert!(!track.device_name.is_empty());
        assert!((track.total_seconds - 6.0).abs() < 0.1);
        assert!((engine.status().total_seconds() - 6.0).abs() < 0.1);
    }

    #[test]
    #[ignore = "precisa de um device de saída real"]
    fn carregar_de_novo_substitui_a_faixa_anterior() {
        let a = "../testdata/t96_24.flac";
        let b = "../testdata/t44_16.flac";
        if !std::path::Path::new(a).exists() || !std::path::Path::new(b).exists() {
            eprintln!("pulando: testdata ausente");
            return;
        }
        let engine = Engine::new();
        engine.load(a).expect("deveria carregar o primeiro");
        let track = engine.load(b).expect("deveria carregar o segundo");
        assert_eq!(track.sample_rate, 44100.0);
        assert_eq!(engine.state(), PlayerState::Loaded);
    }

    #[test]
    #[ignore = "precisa de um device de saída real"]
    fn negociacao_recusada_preserva_a_faixa_carregada() {
        // 192 kHz não está entre os rates do device built-in: a negociação recusa antes de
        // qualquer teardown acontecer.
        let anterior = "../testdata/t96_24.flac";
        let recusado = "../testdata/t192_24.flac";
        if !std::path::Path::new(anterior).exists() || !std::path::Path::new(recusado).exists() {
            eprintln!("pulando: testdata ausente");
            return;
        }
        let engine = Engine::new();
        engine.load(anterior).expect("deveria carregar a primeira faixa");

        assert!(engine.load(recusado).is_err());

        // O que dá valor ao teste: a faixa anterior sobrevive à tentativa recusada, em vez de
        // deixar o engine em Loaded com nada de fato carregado.
        assert_eq!(engine.state(), PlayerState::Loaded);
        assert!((engine.status().total_seconds() - 6.0).abs() < 0.1);

        // `state()` e `total_seconds()` sozinhos não provam nada: teardown() nunca mexe em
        // `self.status`, e a atribuição de `state` só acontece depois da negociação aceitar —
        // então os dois ficam iguais tanto na ordem certa quanto na errada. Só o campo
        // privado `source`, acessível porque `tests` é submódulo de `engine`, revela se o
        // teardown rodou cedo demais e apagou a faixa que devia continuar tocável.
        assert!(
            engine.lock().source.is_some(),
            "a fonte da faixa anterior não pode ser descartada por uma negociação que falhou"
        );
    }
}
