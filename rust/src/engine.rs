//! A casca imperativa da máquina de estados: aqui moram os efeitos sobre o hardware, o
//! arquivo e as threads. As regras de qual transição é permitida ficam em `transitions`,
//! puras e testáveis sem nada disso.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::thread::JoinHandle;
use std::time::Instant;

use coreaudio_sys::{AudioObjectID, AudioStreamBasicDescription};

use crate::tuning::curve::Curve;
use crate::tuning::design::{DEFAULT_TAPS, design};
use crate::tuning::process::Processor;
use crate::tuning::quantize::{OutputFormat, Quantizer};

use crate::device::{
    HoggedDevice, OutputDevice, PendingVolume, live_signals, process_name,
    query_default_output_device,
};
use crate::device_watch::WatchState;
use crate::format::{self, Decision, SampleRateRange};
use crate::playback::Playback;
use crate::recovery::{LiveSignals, Problem, describe, request_applies, verdict};
use crate::source::AudioSource;
use crate::status::SharedStatus;
use crate::timings::{Phase, PhaseTimings, TimingLog};
use crate::transitions::{Command, PlayerState, next_state};
use crate::volume::{VolumeRequest, VolumeUnit, apply_ceiling};

/// O mesmo padrão da CLI: sem `--volume` explícito, o device é baixado para no máximo isto.
pub const DEFAULT_CEILING: f64 = 0.5;

/// Tocando, o IOProc soma frames a cada chamada, mesmo em underrun. Parados por mais que
/// isto, ele não está sendo chamado: é a perda que chega sem notificação.
const STALL_LIMIT: std::time::Duration = std::time::Duration::from_millis(500);

/// A mesma tolerância da confirmação de escrita em `device.rs`: abaixo disto, é o volume que o
/// próprio engine aplicou.
const VOLUME_TOLERANCE: f32 = 0.005;

/// Sem listeners, o intervalo entre conferências ao vivo do device no poll.
const LIVE_CHECK_WITHOUT_WATCH: std::time::Duration = std::time::Duration::from_secs(1);

/// O que a interface precisa saber sobre a faixa recém-carregada.
#[derive(Debug)]
pub struct LoadedTrack {
    pub sample_rate: f64,
    /// `None` na faixa com perda, que não tem profundidade de bits.
    pub bit_depth: Option<u32>,
    /// Só na faixa com perda: o bitrate médio que o arquivo publica.
    pub bitrate_kbps: Option<u32>,
    pub channels: u32,
    pub codec: String,
    pub device_name: String,
    pub total_seconds: f64,
}

/// O que a aplicação de volume de fato fez, para quem quiser relatar.
#[derive(Clone, Debug)]
pub struct VolumeOutcome {
    pub scalar: f32,
    pub decibels: f64,
    pub previous_scalar: f32, // negativo quando o device não expõe volume
    pub lowered_by_ceiling: bool,
    // O teto é rede de proteção, não garantia: quando baixar por ele falha, a reprodução
    // segue (ao contrário de um pedido explícito falhar), mas o motivo não pode se perder —
    // sem este campo ele virava indistinguível de "já estava abaixo do teto".
    pub warning: Option<String>,
}

/// Diagnóstico de linha de comando sobre o device consultado e o que foi negociado com ele.
/// Não é o tipo de fronteira da interface gráfica — `LoadedTrack` é esse; isto é ferramenta
/// de `--info` e de relatório antes de tocar, e não deveria enxugar `LoadedTrack` para caber.
pub struct DeviceReport {
    pub nominal_rate: f64,
    pub supported_rates: Vec<SampleRateRange>,
    pub physical_formats: Vec<AudioStreamBasicDescription>,
    pub volume: f32, // negativo quando o device não expõe volume
    pub negotiated: String,
    pub duplicate_mono_to_stereo: bool,
}

/// Origem de uma falha de `play`: transição inválida, problema da faixa/decodificador ou falha
/// ao tomar/reconfigurar o hardware. A CLI achata as três em `String`, mas a fronteira uniffi
/// usa a distinção para decidir se o auto-avanço deve marcar e pular uma faixa.
pub(crate) enum PlayFailure {
    State(String),
    Track(String),
    Device(String),
}

impl PlayFailure {
    fn into_message(self) -> String {
        match self {
            PlayFailure::State(message)
            | PlayFailure::Track(message)
            | PlayFailure::Device(message) => message,
        }
    }
}

pub(crate) enum LoadFailure {
    Track(String),
    Device(String),
    /// O device retido se perdeu. A faixa pedida ficou carregada no lugar da anterior, sem
    /// device; quem chama aponta a fila para ela.
    DeviceLost(String),
}

impl LoadFailure {
    fn into_message(self) -> String {
        match self {
            LoadFailure::Track(message)
            | LoadFailure::Device(message)
            | LoadFailure::DeviceLost(message) => message,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueueEntry {
    pub id: u64,
    pub path: String,
    pub failure: Option<String>,
}

/// De onde vêm os sinais ao vivo do device. Existe para os testes simularem a perda sem
/// hardware; em produção é sempre o HAL.
pub(crate) trait DeviceProbe: Send + Sync {
    fn signals(&self, device: AudioObjectID, stream: AudioObjectID) -> LiveSignals;
    /// Volume e mudo do device agora; `None` onde não há o controle ou a leitura falhou.
    fn volume(&self, device: AudioObjectID) -> (Option<f32>, Option<bool>);
}

struct HalProbe;

impl DeviceProbe for HalProbe {
    fn signals(&self, device: AudioObjectID, stream: AudioObjectID) -> LiveSignals {
        live_signals(device, stream)
    }

    fn volume(&self, device: AudioObjectID) -> (Option<f32>, Option<bool>) {
        crate::device::live_volume(device)
    }
}

pub struct Engine {
    status: Arc<SharedStatus>,
    inner: Mutex<EngineInner>,
    timings: TimingLog,
    probe: Box<dyn DeviceProbe>,
}

struct EngineInner {
    state: PlayerState,
    path: Option<String>,
    source: Option<AudioSource>,
    device: Option<OutputDevice>,
    decision: Option<Decision>,
    client: Option<AudioStreamBasicDescription>,
    hogged: Option<HoggedDevice>,
    playback: Option<Arc<Playback>>,
    producer: Option<JoinHandle<()>>,
    stop_producer: Arc<AtomicBool>,
    volume: Option<VolumeRequest>,
    /// O device em que o volume foi pedido. `None` vale para qualquer um (o `--volume`).
    volume_uid: Option<String>,
    tuning: Option<Curve>,
    ceiling: f64,
    volume_outcome: Option<VolumeOutcome>,
    queue: Vec<QueueEntry>,
    /// O id da próxima entrada. Só cresce: um id nunca volta, nem depois de limpar a fila.
    next_queue_id: u64,
    current_index: Option<usize>,
    /// Os listeners do device retido e a geração do device já examinada pelo poll.
    watch: Option<Arc<WatchState>>,
    watch_seen: u64,
    /// As gerações de volume e mudo já examinadas pelo poll.
    volume_seen: u64,
    /// Os frames do IOProc na última vez que andaram, e quando.
    watchdog: Option<(u64, Instant)>,
    /// Sem listeners, o poll confere o device ao vivo de tempos em tempos.
    last_live_check: Option<Instant>,
}

impl Engine {
    pub fn new() -> Self {
        Self::with_probe(Box::new(HalProbe))
    }

    pub(crate) fn with_probe(probe: Box<dyn DeviceProbe>) -> Self {
        Self {
            probe,
            status: Arc::new(SharedStatus::new()),
            inner: Mutex::new(EngineInner {
                state: PlayerState::Idle,
                path: None,
                source: None,
                device: None,
                decision: None,
                client: None,
                hogged: None,
                playback: None,
                producer: None,
                stop_producer: Arc::new(AtomicBool::new(false)),
                volume: None,
                volume_uid: None,
                tuning: None,
                ceiling: DEFAULT_CEILING,
                volume_outcome: None,
                queue: Vec::new(),
                next_queue_id: 1,
                current_index: None,
                watch: None,
                watch_seen: 0,
                volume_seen: 0,
                watchdog: None,
                last_live_check: None,
            }),
            timings: TimingLog::default(),
        }
    }

    /// Zera a medição: cada comando da interface é uma transição, e as etapas dele somam.
    pub fn reset_timings(&self) {
        self.timings.reset();
    }

    /// Nunca espera o lock do engine: a medição tem mutex próprio.
    pub fn timings(&self) -> PhaseTimings {
        self.timings.snapshot()
    }

    /// Um pânico dentro de um comando não pode deixar o player inutilizável para sempre: o
    /// estado interno é recuperado em vez de propagar o envenenamento do mutex.
    fn lock(&self) -> MutexGuard<'_, EngineInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Nunca bloqueia: usado por `poll_finished`, que o `snapshot()` da interface chama dez
    /// vezes por segundo. Um comando lento segurando o lock (a aquisição do hog leva
    /// centenas de milissegundos) não pode congelar essa leitura.
    fn try_lock(&self) -> Option<MutexGuard<'_, EngineInner>> {
        match self.inner.try_lock() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    pub fn status(&self) -> Arc<SharedStatus> {
        Arc::clone(&self.status)
    }

    pub fn state(&self) -> PlayerState {
        self.lock().state
    }

    /// Define o volume aplicado quando o device for adquirido. `None` mantém o volume atual,
    /// respeitando o teto.
    /// Liga a afinação com `curve`, ou desliga com `None`. Vale a partir do próximo `play`:
    /// o formato de entrega do decodificador é decidido na partida do stream, e trocá-lo com
    /// o device já tocando é o mesmo que mudar o formato debaixo do IOProc.
    ///
    /// A curva fica como veio. A normalização que impede o ganho positivo de saturar acontece
    /// no desenho do filtro: guardar já normalizada apagaria o preamp, que é justamente o
    /// número que diz quanto a curva pediu a mais do que cabe.
    pub fn set_tuning(&self, curve: Option<Curve>) {
        self.lock().tuning = curve;
    }

    pub fn tuning(&self) -> Option<Curve> {
        self.lock().tuning.clone()
    }

    pub fn set_requested_volume(&self, volume: Option<VolumeRequest>, ceiling: f64) {
        let mut inner = self.lock();
        inner.volume = volume;
        inner.volume_uid = None;
        inner.ceiling = ceiling;
    }

    /// O que a aplicação de volume fez na última aquisição do device, para a CLI relatar.
    pub fn volume_outcome(&self) -> Option<VolumeOutcome> {
        self.lock().volume_outcome.clone()
    }

    /// Diagnóstico para `--info` e para o relatório impresso antes de tocar: o que o device
    /// oferece e o que foi negociado com a faixa carregada. `None` antes de qualquer `load`
    /// bem-sucedido — não há device consultado nem negociação para descrever.
    pub fn device_report(&self) -> Option<DeviceReport> {
        let inner = self.lock();
        let device = inner.device.as_ref()?;
        let decision = inner.decision.as_ref()?;
        Some(DeviceReport {
            nominal_rate: device.nominal_rate,
            supported_rates: device.caps.rates.clone(),
            physical_formats: device.physical_formats.clone(),
            volume: device.volume,
            negotiated: decision.reason.clone(),
            duplicate_mono_to_stereo: decision.duplicate_mono_to_stereo,
        })
    }

    pub fn load(&self, path: &str) -> Result<LoadedTrack, String> {
        self.load_classified(path)
            .map_err(LoadFailure::into_message)
    }

    pub(crate) fn load_classified(&self, path: &str) -> Result<LoadedTrack, LoadFailure> {
        // Abrir o arquivo primeiro, antes de descartar o que estava carregado: se o caminho
        // for inválido, o player continua exatamente como estava.
        let opening = Instant::now();
        let opened = AudioSource::open(path);
        self.timings.record(Phase::Open, opening);
        let source = opened.map_err(LoadFailure::Track)?;
        let file_format = source.format();
        let codec = source.codec_name().to_string();
        let total_frames = source.total_frames();

        // Quando já há um device retido (uma faixa tocando ou pausada), a negociação usa o
        // mesmo `OutputDevice` de novo em vez de perguntar ao sistema: sob hog mode, o "device
        // de saída padrão" que o macOS relata é outro hardware — o mesmo efeito documentado no
        // README (seção Volume) para o volume, e que fez o teste de mutação da Correção 1
        // desta onda precisar sair do nível de `Engine`. Perguntar ao sistema nessa condição
        // faria a faixa nova tocar num device físico diferente do que o usuário está ouvindo
        // (e negociar contra as capacidades erradas). As `caps` guardadas continuam válidas —
        // formatos físicos e taxas suportadas não mudam com o tempo — e o rate, desde a
        // Correção 1, já vem de uma releitura ao vivo dentro de `acquire()`, não daqui.
        //
        // Reaproveitar exige confirmar ao vivo que o device continua vivo e nosso: o auto-avanço
        // e a navegação passam por aqui, e nenhum deles pode seguir tocando em outro device por
        // conta própria.
        let held_device = {
            let mut inner = self.lock();
            if inner.hogged.is_some() {
                if let Some(problem) = self.live_problem(&inner) {
                    let message = self.handle_device_loss(&mut inner, problem);
                    // A faixa pedida fica carregada, sem device: a remoção já moveu a fila para
                    // ela, e o Play seguinte tem de tocar o que a tela mostra.
                    inner.path = Some(path.to_string());
                    inner.source = Some(source);
                    self.status.set_track(total_frames, file_format.sample_rate);
                    return Err(LoadFailure::DeviceLost(message));
                }
                inner.device.clone()
            } else {
                None
            }
        };

        // A negociação corre antes do lock (de novo, se `held_device` for `None`) pelo mesmo
        // motivo de sempre: um arquivo válido cujo formato o device recusa não pode apagar a
        // faixa que já estava carregada e tocável.
        let device = match held_device {
            Some(device) => device,
            None => query_default_output_device().map_err(LoadFailure::Device)?,
        };
        let decision = negotiate_on(&device, &file_format).map_err(LoadFailure::Track)?;

        let mut inner = self.lock();
        // `teardown` só limpa os campos da faixa carregada; soltar o device é
        // responsabilidade só de `stop_and_release`, chamada aqui explicitamente por quem
        // precisa dela (o erro é descartado pelo mesmo motivo de sempre: uma restauração que
        // falhou não pode impedir a carga de uma faixa nova).
        let releasing = Instant::now();
        let _ = self.stop_and_release(&mut inner);
        self.timings.record(Phase::Release, releasing);
        self.teardown(&mut inner);

        let device_name = device.name.clone();
        // Hoje inalcançável: `next_state(_, Load)` é catch-all `Ok` (ver `transitions.rs`), e
        // este `?` nunca dispara. Mas se `Load` um dia puder ser recusado nalgum estado, ele
        // dispararia *depois* de `teardown` já ter apagado a faixa anterior — exatamente o bug
        // que o comentário acima de `stop_and_release`/`teardown` existe para evitar. Quem
        // tornar `Load` falível precisa mover esta checagem para antes do teardown.
        let target = next_state(inner.state, Command::Load)
            .map_err(|e| LoadFailure::Track(e.message.to_string()))?;

        inner.path = Some(path.to_string());
        inner.source = Some(source);
        inner.device = Some(device);
        inner.decision = Some(decision);
        inner.state = target;

        self.status.set_track(total_frames, file_format.sample_rate);
        self.status.set_state(target);

        Ok(LoadedTrack {
            sample_rate: file_format.sample_rate,
            bit_depth: file_format.bit_depth(),
            bitrate_kbps: file_format.bitrate_kbps(),
            channels: file_format.channels,
            codec,
            device_name,
            total_seconds: self.status.total_seconds(),
        })
    }

    pub(crate) fn append_tracks(&self, paths: Vec<String>) -> usize {
        if paths.is_empty() {
            return 0;
        }
        let count = paths.len();
        let mut inner = self.lock();
        for path in paths {
            let id = inner.next_queue_id;
            inner.next_queue_id += 1;
            inner.queue.push(QueueEntry {
                id,
                path,
                failure: None,
            });
        }
        self.status
            .set_queue_state(inner.current_index, inner.queue.len());
        self.status.bump_queue_version();
        count
    }

    pub(crate) fn queue_entries(&self) -> Vec<QueueEntry> {
        self.lock().queue.clone()
    }

    /// Nunca bloqueia, pelo mesmo motivo do `poll_finished`: a interface lê a fila na main
    /// thread, e um comando restaurando ou adquirindo o device segura o lock por segundos.
    /// `None` quer dizer "ocupado agora"; quem chama tenta de novo depois.
    pub(crate) fn try_queue_entries(&self) -> Option<Vec<QueueEntry>> {
        self.try_lock().map(|inner| inner.queue.clone())
    }

    pub(crate) fn queue_position(&self) -> (Option<usize>, usize) {
        let inner = self.lock();
        (inner.current_index, inner.queue.len())
    }

    pub(crate) fn queue_index_of(&self, id: u64) -> Option<usize> {
        self.lock().queue.iter().position(|entry| entry.id == id)
    }

    pub(crate) fn queue_path(&self, index: usize) -> Option<String> {
        self.lock().queue.get(index).map(|entry| entry.path.clone())
    }

    pub(crate) fn set_current_index(&self, index: Option<usize>) {
        let mut inner = self.lock();
        inner.current_index = index;
        self.status.set_queue_state(index, inner.queue.len());
    }

    pub(crate) fn mark_queue_failure(&self, index: usize, failure: Option<String>) {
        let mut inner = self.lock();
        let Some(entry) = inner.queue.get_mut(index) else {
            return;
        };
        if entry.failure == failure {
            return;
        }
        entry.failure = failure;
        self.status.bump_queue_version();
    }

    pub(crate) fn failed_tracks(&self) -> Vec<bool> {
        self.lock()
            .queue
            .iter()
            .map(|entry| entry.failure.is_some())
            .collect()
    }

    pub(crate) fn remove_queue_entry(&self, index: usize) -> Result<(bool, Option<usize>), String> {
        let mut inner = self.lock();
        if index >= inner.queue.len() {
            return Err("índice da fila fora do intervalo".to_string());
        }
        inner.queue.remove(index);
        let (current, reload) =
            crate::queue::after_removal(inner.current_index, index, inner.queue.len());
        let removed_current = inner.current_index == Some(index);
        inner.current_index = current;
        self.status.set_queue_state(current, inner.queue.len());
        self.status.bump_queue_version();
        Ok((removed_current && reload, current))
    }

    pub(crate) fn clear_queue_entries(&self) {
        let mut inner = self.lock();
        if inner.queue.is_empty() && inner.current_index.is_none() {
            return;
        }
        inner.queue.clear();
        inner.current_index = None;
        self.status.set_queue_state(None, 0);
        self.status.bump_queue_version();
    }

    #[cfg(test)]
    pub(crate) fn force_finished_for_test(&self) {
        if let Some(playback) = self.lock().playback.as_ref() {
            playback
                .finished
                .store(true, std::sync::atomic::Ordering::Release);
        }
        self.poll_finished();
    }

    /// Põe o engine num estado que retém um device, sem hardware: o `HoggedDevice` nunca passou
    /// por `acquire`, então soltá-lo não chama o HAL.
    #[cfg(test)]
    pub(crate) fn hold_for_test(
        &self,
        state: PlayerState,
        baseline: crate::recovery::Baseline,
        watch: Arc<WatchState>,
        path: &str,
    ) {
        let mut inner = self.lock();
        inner.path = Some(path.to_string());
        inner.hogged = Some(HoggedDevice::with_baseline_for_test(baseline));
        inner.watch_seen = watch.device_generation();
        inner.volume_seen = volume_generation(&watch);
        inner.watch = Some(watch);
        inner.state = state;
        self.status.set_state(state);
    }

    #[cfg(test)]
    pub(crate) fn holds_device_for_test(&self) -> bool {
        self.lock().hogged.is_some()
    }

    #[cfg(test)]
    pub(crate) fn loaded_path_for_test(&self) -> Option<String> {
        self.lock().path.clone()
    }

    /// O volume que a próxima aquisição vai aplicar, como escalar.
    #[cfg(test)]
    pub(crate) fn requested_volume_for_test(&self) -> Option<f64> {
        self.lock().volume.as_ref().map(|request| request.value)
    }

    /// Faz o watchdog acreditar que os frames estão parados há `ago`.
    #[cfg(test)]
    pub(crate) fn backdate_watchdog_for_test(&self, ago: std::time::Duration) {
        let mut inner = self.lock();
        let frames = self.status.frames_rendered();
        inner.watchdog = Instant::now().checked_sub(ago).map(|since| (frames, since));
    }

    /// Confirma a perda do device ao vivo e, confirmada, solta o que restou dele e volta a
    /// Loaded com a faixa. Devolve a mensagem do problema, ou `None` se o device está bem.
    pub fn recover(&self) -> Option<String> {
        let mut inner = self.lock();
        self.status.set_device_suspect(false);
        let problem = self.live_problem(&inner)?;
        Some(self.handle_device_loss(&mut inner, problem))
    }

    /// O veredito ao vivo do device retido: `None` se nada está retido ou se ele continua
    /// como a aquisição o deixou.
    fn live_problem(&self, inner: &EngineInner) -> Option<Problem> {
        let hogged = inner.hogged.as_ref()?;
        let baseline = hogged.baseline()?;
        let signals = self.probe.signals(hogged.device_id(), hogged.stream_id());
        verdict(&signals, &baseline)
    }

    /// Solta o que restou do device sem escrever nele (a precondição mora em `restore`),
    /// esquece a negociação e volta a Loaded com a faixa. A negociação sai junto porque só um
    /// Play explícito pode escolher outro device.
    fn release_lost_device(&self, inner: &mut EngineInner) {
        // Sem adotar o volume: o que o device tem agora pode ser o de quem tomou o hog.
        let _ = self.release_device(inner, false);
        inner.device = None;
        inner.decision = None;
        inner.client = None;
        self.enter_device_lost_state(inner);
    }

    fn enter_device_lost_state(&self, inner: &mut EngineInner) {
        let target = next_state(inner.state, Command::DeviceLost).unwrap_or(inner.state);
        inner.state = target;
        self.status.set_state(target);
        self.status.reset_progress();
    }

    fn handle_device_loss(&self, inner: &mut EngineInner, problem: Problem) -> String {
        self.release_lost_device(inner);
        let app = match problem {
            Problem::HogLost { owner } if owner > 0 => process_name(owner),
            _ => None,
        };
        let message = describe(problem, app.as_deref());
        self.status.publish_problem(message.clone());
        message
    }

    /// A parte do poll que olha o device. Só lê o HAL quando um listener avisou, quando os
    /// frames pararam tocando, ou — sem listeners — de tempos em tempos. Nunca solta nada:
    /// quem age é o `recover`, fora da main thread.
    fn watch_device(&self, inner: &mut EngineInner) {
        let holding = matches!(
            inner.state,
            PlayerState::Playing | PlayerState::Paused | PlayerState::Finished
        );
        if !holding || inner.hogged.is_none() || self.status.device_suspect() {
            return;
        }
        let notified = match inner.watch.as_ref() {
            Some(watch) => {
                let generation = watch.device_generation();
                let changed = generation != inner.watch_seen;
                inner.watch_seen = generation;
                changed
            }
            None => inner
                .last_live_check
                .is_none_or(|checked| checked.elapsed() >= LIVE_CHECK_WITHOUT_WATCH),
        };
        let stalled = inner.state == PlayerState::Playing && self.frames_stalled(inner);
        if !notified && !stalled {
            return;
        }
        inner.last_live_check = Some(Instant::now());
        if self.live_problem(inner).is_some() {
            self.status.set_device_suspect(true);
        } else if stalled {
            // Rearma: se continuar parado, confere de novo depois de outro `STALL_LIMIT`.
            inner.watchdog = Some((self.status.frames_rendered(), Instant::now()));
        }
    }

    /// A parte do poll que acompanha volume e mudo. Só lê o HAL quando um listener avisou.
    fn watch_volume(&self, inner: &mut EngineInner) {
        let holding = matches!(
            inner.state,
            PlayerState::Playing | PlayerState::Paused | PlayerState::Finished
        );
        if !holding {
            return;
        }
        let (Some(hogged), Some(watch)) = (inner.hogged.as_ref(), inner.watch.as_ref()) else {
            return;
        };
        let generation = volume_generation(watch);
        if generation == inner.volume_seen {
            return;
        }
        inner.volume_seen = generation;
        let device = hogged.device_id();
        self.adopt_live_volume(inner, device);
    }

    /// Volume e mudo lidos agora. Um volume diferente do que o engine aplicou por último foi
    /// mudado por fora, e vira o pedido da próxima faixa: reaplicar o antigo poderia subir o
    /// volume de quem acabou de baixá-lo.
    fn adopt_live_volume(&self, inner: &mut EngineInner, device: AudioObjectID) {
        let (scalar, muted) = self.probe.volume(device);
        if let Some(muted) = muted {
            self.status.set_muted(muted);
        }
        let Some(scalar) = scalar else {
            return;
        };
        if self
            .status
            .volume()
            .is_some_and(|known| (known - scalar).abs() < VOLUME_TOLERANCE)
        {
            return;
        }
        inner.volume = Some(VolumeRequest {
            valid: true,
            unit: VolumeUnit::Percent,
            value: scalar as f64,
            reason: String::new(),
        });
        inner.volume_uid = inner.device.as_ref().map(|device| device.uid.clone());
        self.status.set_volume(scalar);
    }

    fn frames_stalled(&self, inner: &mut EngineInner) -> bool {
        let frames = self.status.frames_rendered();
        match inner.watchdog {
            Some((seen, since)) if seen == frames => since.elapsed() > STALL_LIMIT,
            _ => {
                inner.watchdog = Some((frames, Instant::now()));
                false
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn current_device_id_for_test(&self) -> Option<u32> {
        self.lock().device.as_ref().map(|device| device.id)
    }

    pub fn play(&self) -> Result<(), String> {
        self.play_classified().map_err(PlayFailure::into_message)
    }

    /// Mesma lógica de `play`, mas preserva se a falha veio da transição de estado ou do
    /// hardware — ver o comentário em `PlayFailure` para o porquê de existir separado de
    /// `play`.
    pub(crate) fn play_classified(&self) -> Result<(), PlayFailure> {
        let mut inner = self.lock();
        let target = next_state(inner.state, Command::Play)
            .map_err(|e| PlayFailure::State(e.message.to_string()))?;

        match inner.state {
            PlayerState::Playing => return Ok(()), // idempotente: duplo clique no botão
            PlayerState::Paused => match self.live_problem(&inner) {
                None => {
                    // O device já deveria estar retido neste estado; sem ele, o problema não é o
                    // hardware ter recusado algo, é o player estar num estado que ele mesmo não
                    // sabe mais resolver — por isso `State`, não `Device`.
                    let hogged = inner.hogged.as_mut().ok_or_else(|| {
                        PlayFailure::State("pausado sem device; recarregue a faixa".to_string())
                    })?;
                    hogged.resume().map_err(PlayFailure::Device)?;
                }
                // Mudou enquanto pausado: o Mac dormiu, outro app mexeu no formato. Recomeça
                // do zero no mesmo device. Se a partida falhar, o device já foi solto, e ficar
                // pausado sem ele seria um beco sem saída.
                Some(Problem::FormatChanged) => {
                    if let Err(failure) = self.start_common(&mut inner, true) {
                        self.enter_device_lost_state(&mut inner);
                        return Err(failure);
                    }
                }
                // O device sumiu ou deixou de ser nosso. Este é um Play explícito, o único que
                // pode negociar com o device padrão atual.
                Some(Problem::Disconnected | Problem::HogLost { .. }) => {
                    self.release_lost_device(&mut inner);
                    self.start_common(&mut inner, true)?;
                }
            },
            PlayerState::Loaded | PlayerState::Finished => self.start_common(&mut inner, true)?,
            // O `next_state` acima já teria retornado erro para Play a partir de Idle ou
            // Failed. Um wildcard aqui esconderia uma variante nova de `PlayerState` caindo
            // silenciosamente em `start_common`; exaustivo, o compilador força revisar este
            // match a cada variante adicionada.
            PlayerState::Idle | PlayerState::Failed => {
                unreachable!("next_state já filtrou Play a partir de {:?}", inner.state)
            }
        }

        inner.state = target;
        self.status.set_state(target);
        Ok(())
    }

    pub fn pause(&self) -> Result<(), String> {
        let mut inner = self.lock();
        let target = next_state(inner.state, Command::Pause).map_err(|e| e.message.to_string())?;
        if let Some(hogged) = inner.hogged.as_mut() {
            hogged.stop();
        }
        inner.state = target;
        self.status.set_state(target);
        Ok(())
    }

    pub fn set_volume(&self, scalar: f32) -> Result<(), String> {
        let mut inner = self.lock();
        // Guardado sempre, não só quando falta o device: se `hogged` está com o device na mão
        // agora, a faixa pode terminar e a próxima aquisição começa chamando
        // `stop_and_release`, que restaura o volume pré-hog — sem este pedido persistido,
        // `apply_volume` rodaria com `inner.volume == None` e aplicaria só o teto sobre essa
        // leitura antiga, descartando o que o usuário escolheu (e podendo até subir o volume).
        // `value` é escalar de 0 a 1 quando a unidade é porcentagem — não 0 a 100.
        inner.volume = Some(VolumeRequest {
            valid: true,
            unit: VolumeUnit::Percent,
            value: scalar as f64,
            reason: String::new(),
        });
        inner.volume_uid = inner.device.as_ref().map(|device| device.uid.clone());
        // Sob suspeita, o device pode já ser de outro app: o pedido fica guardado para a
        // próxima aquisição, sem escrever nele.
        let suspect = self.status.device_suspect();
        match inner.hogged.as_mut().filter(|_| !suspect) {
            Some(hogged) => {
                // Interativa: o device já está configurado e estável aqui, então esta escrita
                // confere na hora em vez de esperar 30 ms como a da aquisição. O slider chama
                // isto a cada quadro do arrasto — ver `HoggedDevice::write_volume_now`.
                hogged.set_volume_interactive(scalar)?;
                if let Some((current, _)) = hogged.read_volume() {
                    self.status.set_volume(current);
                } else {
                    self.status.set_volume(scalar);
                }
            }
            None => {
                self.status.set_volume(scalar);
            }
        }
        Ok(())
    }

    /// A interface chama isto a cada poll: o fim da faixa é detectado pelo IOProc, que não
    /// pode mudar o estado do engine por estar em thread de tempo real.
    pub fn poll_finished(&self) {
        let Some(mut inner) = self.try_lock() else {
            // Um comando (play/pause/shutdown) está com o lock; ele muda o estado sozinho, e
            // o próximo poll, 100 ms depois, pega o fim da faixa se for o caso.
            return;
        };
        let terminou = inner.state == PlayerState::Playing
            && inner
                .playback
                .as_ref()
                .is_some_and(|p| p.finished.load(std::sync::atomic::Ordering::Acquire));
        if terminou {
            if let Some(hogged) = inner.hogged.as_mut() {
                hogged.stop();
            }
            inner.state = PlayerState::Finished;
            self.status.set_state(PlayerState::Finished);
        }
        self.watch_device(&mut inner);
        self.watch_volume(&mut inner);
    }

    pub fn shutdown(&self) -> Result<(), String> {
        let mut inner = self.lock();
        let restore = self.stop_and_release(&mut inner);
        self.teardown(&mut inner);
        inner.state = PlayerState::Idle;
        self.status.set_state(PlayerState::Idle);
        restore
    }

    fn start_common(
        &self,
        inner: &mut EngineInner,
        attach_io_proc: bool,
    ) -> Result<(), PlayFailure> {
        // Incondicional, mesmo vindo de Loaded (onde nada está retido — vira no-op). O match
        // de `play()` só chega aqui a partir de Loaded ou Finished; vindo de Finished um
        // HoggedDevice antigo pode continuar registrado (o fim da faixa só chama
        // `hogged.stop()`, não solta o device) — criar um segundo aqui sobreporia
        // hog/rate/formato por cima do que a acquire() nova ainda vai gravar como "original",
        // e o Drop do antigo revogaria hog mode e reverteria rate/formato por baixo de uma
        // sessão que se acredita exclusiva — tudo retornando noErr, sem nenhum sinal de erro.
        let releasing = Instant::now();
        let _ = self.stop_and_release(inner);
        self.timings.record(Phase::Release, releasing);

        // Recarrega do início: vindo de Finished o decodificador já se esgotou, e mesmo
        // vindo de Loaded é preciso um AudioSource que possa ser movido para a produtora.
        let path = inner
            .path
            .clone()
            .ok_or_else(|| PlayFailure::State("nenhum arquivo carregado".to_string()))?;
        let reopening = Instant::now();
        let mut source = AudioSource::open(&path).map_err(PlayFailure::Track)?;
        self.timings.record(Phase::Reopen, reopening);
        let file_format = source.format();

        // Depois de perder o device a negociação foi esquecida. Só um Play explícito chega aqui
        // assim, e é ele que escolhe o device padrão atual; a recusa dele é do device, não da
        // faixa, que não pode ser marcada como falha por isso.
        if inner.device.is_none() || inner.decision.is_none() {
            let device = query_default_output_device().map_err(PlayFailure::Device)?;
            let decision = negotiate_on(&device, &file_format).map_err(PlayFailure::Device)?;
            inner.device = Some(device);
            inner.decision = Some(decision);
        }

        // Um volume pedido para outro device não vale neste: vira o teto.
        let volume_elsewhere = match (inner.volume_uid.as_deref(), inner.device.as_ref()) {
            (Some(uid), Some(device)) if !request_applies(Some(uid), &device.uid) => {
                Some(device.name.clone())
            }
            _ => None,
        };
        let volume_warning = volume_elsewhere.map(|name| {
            inner.volume = None;
            inner.volume_uid = None;
            format!(
                "a saída agora é {name}: o volume pedido era para outro device e voltou ao \
                 teto de {:.0}%",
                inner.ceiling * 100.0
            )
        });

        // Os dados que o resto da função precisa são copiados aqui para que os empréstimos
        // de `inner` terminem antes das atribuições no fim: sem isso o verificador de
        // empréstimos recusa a função.
        let device = inner
            .device
            .as_ref()
            .ok_or_else(|| PlayFailure::State("device não consultado".to_string()))?;
        let decision = inner
            .decision
            .as_ref()
            .ok_or_else(|| PlayFailure::State("formato não negociado".to_string()))?;

        let acquiring = Instant::now();
        // Registrados antes do acquire, para as esperas dele acordarem na notificação e nada
        // que mude depois passar despercebido. Falhar aqui não impede de tocar: sem listeners,
        // as esperas conferem no intervalo e o poll confere o device de tempos em tempos.
        let watch = crate::device_watch::watch(device.id, device.stream_id).ok();
        // Tiradas antes do acquire: uma mudança por fora durante o prefill não pode se perder.
        // As notificações do nosso próprio acquire custam só uma conferência no poll.
        let seen_generations = watch
            .as_ref()
            .map(|watch| (watch.device_generation(), volume_generation(watch)));
        let mut hogged = HoggedDevice::with_watch(watch.clone());
        let physical = device.physical_formats[decision.physical_format_index as usize];
        hogged
            .acquire(device, decision.sample_rate, &physical)
            .map_err(PlayFailure::Device)?;
        self.timings.record(Phase::Acquire, acquiring);

        // Dali em diante, nenhum erro pode propagar com um `?` solto: `hogged` ainda é local,
        // não movida para `inner.hogged`, e um `?` a derrubaria — o `Drop::restore()` rodaria,
        // mas `restore_error` seria descartado junto, e como `inner.hogged` continuaria `None`
        // um `shutdown()` posterior cairia no ramo `None => Ok(())` e mentiria sucesso mesmo
        // com a restauração tendo falhado. `finish_with_error` fecha essa fresta: finaliza
        // `hogged` explicitamente e concatena os dois erros, para nenhum se perder.

        // Com o device já nosso, logo depois do formato: a escrita sai aqui, na ordem de
        // sempre. A conferência, que precisa de 30 ms para poder discordar, fica para antes do
        // start — nenhuma amostra sai antes dela —, e a espera corre junto com o prefill. A
        // publicação no `SharedStatus` fica para o fim da função — ver o comentário perto de
        // `self.status.set_volume` mais abaixo.
        let applying_volume = Instant::now();
        let volume_start =
            match begin_apply_volume(&mut hogged, device, inner.volume.as_ref(), inner.ceiling) {
                Ok(start) => start,
                Err(error) => {
                    return Err(PlayFailure::Device(finish_with_error(hogged, error)));
                }
            };
        self.timings.record(Phase::Volume, applying_volume);

        let stream = hogged.stream_format();
        if stream.mFormatID != coreaudio_sys::kAudioFormatLinearPCM {
            return Err(PlayFailure::Device(finish_with_error(
                hogged,
                "o device não está em PCM linear; não vou alimentá-lo".to_string(),
            )));
        }

        let (client, non_interleaved) = crate::engine::client_format_for(&stream);
        let check = crate::format::validate_interleaved_format(
            client.mBitsPerChannel,
            client.mBytesPerFrame,
            client.mChannelsPerFrame,
        );
        if !check.ok {
            return Err(PlayFailure::Device(finish_with_error(
                hogged,
                format!("formato de entrega inconsistente: {}", check.reason),
            )));
        }

        // A afinação muda o que o decodificador entrega: float32, para o filtro trabalhar sem
        // desempacotar inteiro e sem perder resolução no caminho. Sem afinação a entrega
        // continua sendo exatamente o formato do device, e nenhum byte é tocado entre o
        // decodificador e o DAC.
        let tuning = inner.tuning.clone();
        let delivery = match tuning {
            Some(_) => float_delivery_for(&client),
            None => client,
        };

        if let Err(error) = source.set_client_format(&delivery) {
            return Err(PlayFailure::Track(finish_with_error(hogged, error)));
        }

        // O decodificador pode ajustar o que aceitou. Divergência aqui é a diferença entre
        // silêncio e ruído em volume total.
        let effective = match source.effective_client_format() {
            Ok(effective) => effective,
            Err(error) => return Err(PlayFailure::Track(finish_with_error(hogged, error))),
        };
        if let Err(error) = crate::engine::assert_same_delivery(&effective, &delivery) {
            return Err(PlayFailure::Track(finish_with_error(hogged, error)));
        }

        let ring_bytes = client.mSampleRate as usize * client.mBytesPerFrame as usize * 2;
        self.status
            .set_track(source.total_frames(), file_format.sample_rate);
        let playback = Arc::new(Playback::new(
            ring_bytes,
            &client,
            non_interleaved,
            Arc::clone(&self.status),
        ));

        let tuned = match tuning {
            None => None,
            Some(curve) => {
                match build_tuning(&curve, &client) {
                    Ok(pair) => Some(pair),
                    // Filtro que não se desenha é erro de partida, não de faixa: o arquivo
                    // está bom, quem não serve é a curva ou o formato do device.
                    Err(error) => {
                        return Err(PlayFailure::Device(finish_with_error(hogged, error)));
                    }
                }
            }
        };

        let stop = Arc::new(AtomicBool::new(false));
        let producer = {
            let playback = Arc::clone(&playback);
            let stop = Arc::clone(&stop);
            let status = Arc::clone(&self.status);
            let device_bytes_per_frame = client.mBytesPerFrame as usize;
            let channels = client.mChannelsPerFrame as usize;
            std::thread::spawn(move || {
                match tuned {
                    None => produce_direct(&mut source, &playback, &stop, device_bytes_per_frame),
                    Some((processor, quantizer)) => produce_tuned(
                        &mut source,
                        &playback,
                        &stop,
                        &status,
                        TunedProduction {
                            processor,
                            quantizer,
                            channels,
                            device_bytes_per_frame,
                        },
                    ),
                }
                playback
                    .producer_done
                    .store(true, std::sync::atomic::Ordering::Release);
            })
        };

        // Deixa o buffer encher antes de abrir o fluxo, para o começo não sair picotado.
        let prefilling = Instant::now();
        let half = playback.ring.capacity() / 2;
        for _ in 0..200 {
            if playback.ring.available_to_read() >= half
                || playback
                    .producer_done
                    .load(std::sync::atomic::Ordering::Acquire)
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        self.timings.record(Phase::Prefill, prefilling);

        let confirming_volume = Instant::now();
        let outcome = match finish_apply_volume(&hogged, volume_start) {
            Ok(outcome) => outcome,
            Err(error) => {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = producer.join();
                return Err(PlayFailure::Device(finish_with_error(hogged, error)));
            }
        };
        self.timings.record(Phase::Volume, confirming_volume);

        // O modo dump usa exatamente este mesmo caminho, sem só esta última etapa: assim ele
        // percorre decodificador, ring buffer e alinhamento de frame, que é onde estiveram os
        // bugs mais caros. Um atalho que apenas decodificasse não provaria nada disso.
        if attach_io_proc {
            // SAFETY: o io_proc desreferencia este ponteiro cru, sem contagem de referência, a
            // cada callback de tempo real do Core Audio. O invariante que sustenta isso: ao
            // menos um `Arc<Playback>` fica vivo enquanto o callback puder disparar.
            // `stop_and_release` garante a ordem inversa na saída — chama `hogged.stop()`
            // (nenhum callback novo depois disso), junta a produtora, destrói o IOProc e só
            // então zera `inner.playback`, então o ponteiro nunca é lido depois do `Arc` cair.
            // Se o stop e o destroy falharem, o `Playback` é vazado em vez de liberado.
            let context = Arc::as_ptr(&playback) as *mut std::ffi::c_void;
            let starting = Instant::now();
            let started = unsafe { hogged.start(Some(crate::playback::io_proc), context) };
            self.timings.record(Phase::Start, starting);
            if let Err(error) = started {
                // A produtora e o pré-buffer já rodaram; sem isto, ninguém mais teria a flag
                // `stop` para sinalizar parada, e a thread giraria para sempre enchendo um
                // ring buffer que nenhum consumidor vai esvaziar.
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = producer.join();
                return Err(PlayFailure::Device(finish_with_error(hogged, error)));
            }
        }

        // Só agora nada mais nesta função pode falhar e derrubar o `HoggedDevice` local (cujo
        // `Drop` reverteria o volume no hardware) — publicar antes deixaria o status
        // mostrando um volume que o device já não tem mais caso alguma checagem acima falhe.
        self.status.set_volume(outcome.scalar);
        let (_, muted) = self.probe.volume(hogged.device_id());
        self.status.set_muted(muted.unwrap_or(false));
        inner.volume_outcome = Some(outcome);
        inner.client = Some(client);
        inner.hogged = Some(hogged);
        inner.playback = Some(playback);
        inner.producer = Some(producer);
        inner.stop_producer = stop;
        (inner.watch_seen, inner.volume_seen) = seen_generations.unwrap_or_default();
        inner.watch = watch;
        inner.watchdog = None;
        inner.last_live_check = None;
        self.status.set_device_suspect(false);
        self.status.clear_problem();
        if let Some(warning) = volume_warning {
            self.status.publish_problem(warning);
        }
        Ok(())
    }

    /// Prepara tudo como o `play`, menos o IOProc, e devolve o que o consumidor de disco
    /// precisa. Usado pelo `--dump`.
    pub fn start_offline(&self) -> Result<(Arc<Playback>, AudioStreamBasicDescription), String> {
        let mut inner = self.lock();
        let target = next_state(inner.state, Command::Play).map_err(|e| e.message.to_string())?;
        self.start_common(&mut inner, false)
            .map_err(PlayFailure::into_message)?;
        let playback = inner
            .playback
            .clone()
            .ok_or_else(|| "playback não foi criado".to_string())?;
        let client = inner
            .client
            .ok_or_else(|| "formato de entrega não foi definido".to_string())?;
        inner.state = target;
        self.status.set_state(target);
        Ok((playback, client))
    }

    fn stop_and_release(&self, inner: &mut EngineInner) -> Result<(), String> {
        self.release_device(inner, true)
    }

    /// `adopt_volume` falso quando o device se perdeu: o volume dele já não é o nosso.
    fn release_device(&self, inner: &mut EngineInner, adopt_volume: bool) -> Result<(), String> {
        if let Some(hogged) = inner.hogged.as_mut() {
            hogged.stop();
        }
        inner
            .stop_producer
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(producer) = inner.producer.take() {
            let _ = producer.join();
        }
        // A última chance de ver uma mudança de volume cuja notificação não chegou: a
        // restauração devolve o volume de antes do hog, e a mudança se perderia.
        let acquired = inner
            .hogged
            .as_ref()
            .filter(|hogged| adopt_volume && hogged.baseline().is_some())
            .map(HoggedDevice::device_id);
        if let Some(device) = acquired {
            self.adopt_live_volume(inner, device);
        }
        // O IOProc é destruído em `finish`, antes de o `Arc<Playback>` cair: o callback lê o
        // ponteiro cru dele. Se nem o stop nem o destroy funcionaram, o callback pode
        // continuar vivo, e o `Playback` fica vazado de propósito.
        let restore = match inner.hogged.as_mut() {
            Some(hogged) => hogged.finish(),
            None => Ok(()),
        };
        if inner
            .hogged
            .as_ref()
            .is_some_and(HoggedDevice::io_proc_may_still_run)
            && let Some(playback) = inner.playback.as_ref()
        {
            std::mem::forget(Arc::clone(playback));
        }
        inner.playback = None;
        inner.hogged = None;
        inner.watch = None;
        inner.watchdog = None;
        restore
    }

    /// Descarta os campos da faixa carregada. Não toca no device: quem chama decide se e
    /// quando soltar o hardware, chamando `stop_and_release` — uma única responsável em vez
    /// de duas funções que precisam ser lidas juntas para confirmar que é seguro.
    fn teardown(&self, inner: &mut EngineInner) {
        inner.source = None;
        inner.decision = None;
        inner.device = None;
        inner.client = None;
        inner.path = None;
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Engine {
    /// Rede de segurança para os caminhos que não passam por `shutdown` — uma exceção não
    /// tratada no app, por exemplo. Não cobre `kill -9`: nesse caso o sistema solta o hog
    /// sozinho, mas o sample rate fica trocado.
    fn drop(&mut self) {
        let mut inner = self.lock();
        let _ = self.stop_and_release(&mut inner);
    }
}

fn volume_generation(watch: &WatchState) -> u64 {
    use crate::device_watch::Watched;
    watch.generation(Watched::Volume) + watch.generation(Watched::Mute)
}

/// A negociação da carga e a do Play depois de perder o device são a mesma: a diferença é só
/// como cada uma classifica a recusa. `Err` traz o motivo.
fn negotiate_on(
    device: &OutputDevice,
    file_format: &format::FileFormat,
) -> Result<Decision, String> {
    let decision = format::negotiate(file_format, &device.caps);
    if decision.play {
        Ok(decision)
    } else {
        Err(decision.reason)
    }
}

/// Consome o `HoggedDevice` e concatena ao erro original o que a restauração relatar, para que
/// um erro entre `acquire()` e a atribuição a `inner.hogged` nunca engula o outro — ver o
/// comentário em `start_common` sobre por que um `?` solto ali é o bug.
fn finish_with_error(mut hogged: HoggedDevice, error: String) -> String {
    match hogged.finish() {
        Ok(()) => error,
        Err(restore_error) => format!("{error}; além disso, a restauração falhou: {restore_error}"),
    }
}

/// O decodificador entrega sempre intercalado, que é como o ring buffer guarda; o IOProc
/// desintercala se o device pedir assim.
///
/// O tamanho do frame vem do próprio device, nunca de bitsPerChannel/8: um formato pode
/// carregar amostras de 24 bits em containers de 32, e presumir empacotamento faria o
/// decodificador produzir um passo e o IOProc ler outro — ruído branco, não música.
/// Quantos frames a produtora processa por vez no modo afinado. Não é latência de saída — o
/// ring buffer fica entre isto e o DAC — só o tamanho do bloco da convolução.
const TUNED_BLOCK_FRAMES: usize = 8192;

struct TunedProduction {
    processor: Processor,
    quantizer: Quantizer,
    channels: usize,
    device_bytes_per_frame: usize,
}

/// Entrega em float32 intercalado, mantendo taxa e canais do device. É o formato em que o
/// filtro trabalha; o que sai dele volta para o formato do device no `Quantizer`.
fn float_delivery_for(client: &AudioStreamBasicDescription) -> AudioStreamBasicDescription {
    let mut delivery = *client;
    delivery.mFormatFlags = coreaudio_sys::kAudioFormatFlagIsFloat
        | coreaudio_sys::kAudioFormatFlagIsPacked
        | coreaudio_sys::kAudioFormatFlagsNativeEndian;
    delivery.mBitsPerChannel = 32;
    delivery.mBytesPerFrame = 4 * client.mChannelsPerFrame;
    delivery.mBytesPerPacket = delivery.mBytesPerFrame;
    delivery.mFramesPerPacket = 1;
    delivery
}

/// O formato de destino da requantização, lido do que o device de fato publica — nunca
/// deduzido de `bitsPerChannel / 8`, porque amostras de 24 bits moram em containers de 32.
fn output_format_for(client: &AudioStreamBasicDescription) -> OutputFormat {
    let is_float = client.mFormatFlags & coreaudio_sys::kAudioFormatFlagIsFloat != 0;
    OutputFormat {
        sample_type: if is_float {
            crate::format::SampleType::Float
        } else {
            crate::format::SampleType::Integer
        },
        bits_per_channel: client.mBitsPerChannel,
        bytes_per_sample: client.mBytesPerFrame / client.mChannelsPerFrame,
    }
}

fn build_tuning(
    curve: &Curve,
    client: &AudioStreamBasicDescription,
) -> Result<(Processor, Quantizer), String> {
    // Normaliza aqui, e não na entrada: ganho positivo em digital satura, e o único jeito de
    // realizar a forma da curva é descer tudo até o pico encostar em 0 dBFS.
    let fir = design(&curve.normalized(), client.mSampleRate, DEFAULT_TAPS)?;
    let processor = Processor::new(&fir, client.mChannelsPerFrame as usize, TUNED_BLOCK_FRAMES)?;
    // A semente vem do relógio para que duas execuções não somem exatamente o mesmo ruído de
    // dither ao mesmo material — o que o tornaria, na prática, parte do sinal.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x2545_F491_4F6C_DD1D);
    let quantizer = Quantizer::new(output_format_for(client), seed)?;
    Ok((processor, quantizer))
}

/// Escreve tudo no ring, cedendo a vez enquanto o consumidor não abre espaço. Devolve `false`
/// se a parada foi pedida no meio.
fn write_all_to_ring(playback: &Playback, stop: &AtomicBool, bytes: &[u8]) -> bool {
    let mut written = 0usize;
    while written < bytes.len() {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        written += playback.ring.write(&bytes[written..]);
        if written < bytes.len() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    true
}

/// Modo bit-perfect: do decodificador para o ring, sem tocar em nenhum byte.
fn produce_direct(
    source: &mut AudioSource,
    playback: &Playback,
    stop: &AtomicBool,
    bytes_per_frame: usize,
) {
    let mut chunk = vec![0u8; 64 * 1024];
    let frames_per_chunk = (chunk.len() / bytes_per_frame) as u32;
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        let got = source.read(&mut chunk, frames_per_chunk);
        if got == 0 {
            break;
        }
        let bytes = got as usize * bytes_per_frame;
        if !write_all_to_ring(playback, stop, &chunk[..bytes]) {
            return;
        }
    }
}

/// Modo afinado: decodificador em float32, filtro, volta ao formato do device, ring.
fn produce_tuned(
    source: &mut AudioSource,
    playback: &Playback,
    stop: &AtomicBool,
    status: &SharedStatus,
    mut production: TunedProduction,
) {
    let channels = production.channels;
    let mut raw = vec![0u8; TUNED_BLOCK_FRAMES * channels * 4];
    let mut samples = vec![0f32; TUNED_BLOCK_FRAMES * channels];
    let mut out = vec![0u8; TUNED_BLOCK_FRAMES * production.device_bytes_per_frame];
    let mut published_clips = 0u64;

    loop {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let got = source.read(&mut raw, TUNED_BLOCK_FRAMES as u32);
        if got == 0 {
            break;
        }
        let count = got as usize * channels;
        decode_float32(&raw[..count * 4], &mut samples[..count]);

        if !filter_and_write(
            playback,
            stop,
            &mut production,
            &mut samples[..count],
            &mut out,
        ) {
            return;
        }
        published_clips = publish_clips(status, &production, published_clips);
    }

    // A cauda é a resposta do filtro às últimas amostras da faixa. Sem drenar, o fim é cortado
    // — não o silêncio depois dele, mas o decaimento das últimas notas.
    let tail = production.processor.tail_frames();
    let mut drained = 0usize;
    while drained < tail {
        let frames = (tail - drained).min(TUNED_BLOCK_FRAMES);
        let count = frames * channels;
        production.processor.drain_tail(&mut samples[..count]);
        let Ok(bytes) = production.quantizer.write(&samples[..count], &mut out) else {
            return;
        };
        if !write_all_to_ring(playback, stop, &out[..bytes]) {
            return;
        }
        drained += frames;
    }
    publish_clips(status, &production, published_clips);
}

fn decode_float32(raw: &[u8], samples: &mut [f32]) {
    let (words, _) = raw.as_chunks::<4>();
    for (sample, bytes) in samples.iter_mut().zip(words) {
        *sample = f32::from_le_bytes(*bytes);
    }
}

fn filter_and_write(
    playback: &Playback,
    stop: &AtomicBool,
    production: &mut TunedProduction,
    samples: &mut [f32],
    out: &mut [u8],
) -> bool {
    if production.processor.process(samples).is_err() {
        return false;
    }
    let Ok(bytes) = production.quantizer.write(samples, out) else {
        return false;
    };
    write_all_to_ring(playback, stop, &out[..bytes])
}

fn publish_clips(status: &SharedStatus, production: &TunedProduction, published: u64) -> u64 {
    let total = production.processor.clipped() + production.quantizer.clipped();
    if total > published {
        status.add_clipped(total - published);
    }
    total
}

pub(crate) fn client_format_for(
    stream: &AudioStreamBasicDescription,
) -> (AudioStreamBasicDescription, bool) {
    let non_interleaved =
        stream.mFormatFlags & coreaudio_sys::kAudioFormatFlagIsNonInterleaved != 0;
    let mut client = *stream;
    client.mFormatFlags &= !coreaudio_sys::kAudioFormatFlagIsNonInterleaved;
    client.mFramesPerPacket = 1;
    client.mBytesPerFrame = if non_interleaved {
        stream.mBytesPerFrame * stream.mChannelsPerFrame
    } else {
        stream.mBytesPerFrame
    };
    client.mBytesPerPacket = client.mBytesPerFrame;
    (client, non_interleaved)
}

pub(crate) fn assert_same_delivery(
    effective: &AudioStreamBasicDescription,
    client: &AudioStreamBasicDescription,
) -> Result<(), String> {
    let float = coreaudio_sys::kAudioFormatFlagIsFloat;
    if effective.mBitsPerChannel != client.mBitsPerChannel
        || effective.mBytesPerFrame != client.mBytesPerFrame
        || effective.mChannelsPerFrame != client.mChannelsPerFrame
        || (effective.mFormatFlags & float) != (client.mFormatFlags & float)
        || (effective.mSampleRate - client.mSampleRate).abs() > 0.5
    {
        return Err(
            "o decodificador vai entregar um formato diferente do que o device espera; \
             reproduzir assim geraria ruído"
                .to_string(),
        );
    }
    Ok(())
}

/// Lê o volume de fato assumido pelo hardware; sem isso, cai no valor que se tentou aplicar
/// (ainda a melhor estimativa) com decibéis marcados como desconhecidos via NaN.
fn read_applied(hogged: &HoggedDevice, fallback_scalar: f32) -> (f32, f64) {
    match hogged.read_volume() {
        Some((scalar, decibels)) => (scalar, decibels),
        None => (fallback_scalar, f64::NAN),
    }
}

/// A escrita de volume da aquisição: já decidida, ou escrita e à espera da conferência.
pub(crate) enum VolumeStart {
    Decided(VolumeOutcome),
    Pending {
        pending: PendingVolume,
        /// Um pedido explícito é garantia: se a conferência falhar, não toca.
        explicit: bool,
        previous_scalar: f32,
        /// O volume antes da escrita, para o aviso quando o teto não pega.
        current: f32,
    },
}

/// Um pedido explícito é uma garantia: se não der para cumprir, é melhor não tocar do que
/// tocar mais alto do que se pediu. Já o teto é uma rede de proteção — não havendo controle
/// de volume, ou falhando a escrita, avisa e segue, que é o comportamento de sempre.
///
/// Esta é a primeira metade: decide e escreve. A conferência é `finish_apply_volume`.
pub(crate) fn begin_apply_volume(
    hogged: &mut HoggedDevice,
    device: &OutputDevice,
    request: Option<&VolumeRequest>,
    ceiling: f64,
) -> Result<VolumeStart, String> {
    if let Some(request) = request {
        let scalar = match request.unit {
            VolumeUnit::Percent => request.value as f32,
            VolumeUnit::Decibels => hogged
                .decibels_to_scalar(request.value)
                .ok_or("este device não converte decibéis; use porcentagem")?,
        };
        let pending = hogged.begin_volume(scalar)?;
        return Ok(VolumeStart::Pending {
            pending,
            explicit: true,
            previous_scalar: device.volume,
            current: device.volume,
        });
    }

    if device.volume < 0.0 {
        return Ok(VolumeStart::Decided(VolumeOutcome {
            scalar: -1.0,
            decibels: f64::NAN,
            previous_scalar: device.volume,
            lowered_by_ceiling: false,
            warning: None,
        }));
    }

    // O teto decide sobre o volume lido agora, não sobre o que havia antes de tomar o
    // device: entre uma coisa e outra o usuário pode ter mexido no volume, e a
    // reconfiguração do device também pode alterá-lo. Uma proteção que age sobre leitura
    // velha não protege.
    let current = hogged
        .read_volume()
        .map(|(scalar, _)| scalar)
        .unwrap_or(device.volume);

    let decision = apply_ceiling(current as f64, ceiling);
    if !decision.apply {
        let (applied_scalar, applied_decibels) = read_applied(hogged, current);
        return Ok(VolumeStart::Decided(VolumeOutcome {
            scalar: applied_scalar,
            decibels: applied_decibels,
            previous_scalar: current,
            lowered_by_ceiling: false,
            warning: None,
        }));
    }

    match hogged.begin_volume(decision.scalar as f32) {
        Ok(pending) => Ok(VolumeStart::Pending {
            pending,
            explicit: false,
            previous_scalar: current,
            current,
        }),
        Err(error) => Ok(VolumeStart::Decided(ceiling_not_applied(
            hogged, current, &error,
        ))),
    }
}

/// A segunda metade: confere a escrita de `begin_apply_volume` e lê o que o device assumiu.
pub(crate) fn finish_apply_volume(
    hogged: &HoggedDevice,
    start: VolumeStart,
) -> Result<VolumeOutcome, String> {
    let (pending, explicit, previous_scalar, current) = match start {
        VolumeStart::Decided(outcome) => return Ok(outcome),
        VolumeStart::Pending {
            pending,
            explicit,
            previous_scalar,
            current,
        } => (pending, explicit, previous_scalar, current),
    };
    if hogged.confirm_volume(&pending) {
        let (applied_scalar, applied_decibels) = read_applied(hogged, pending.target());
        return Ok(VolumeOutcome {
            scalar: applied_scalar,
            decibels: applied_decibels,
            previous_scalar,
            lowered_by_ceiling: !explicit,
            warning: None,
        });
    }
    let error = "o device não assumiu o volume pedido; não vou tocar sem essa garantia";
    if explicit {
        return Err(error.to_string());
    }
    Ok(ceiling_not_applied(hogged, current, error))
}

/// O teto é rede de proteção, não garantia: não conseguir baixar não pode abortar a
/// reprodução, ao contrário de um pedido explícito falhar. O texto do erro vai no aviso para
/// não ficar indistinguível de "já estava abaixo do teto".
fn ceiling_not_applied(hogged: &HoggedDevice, current: f32, error: &str) -> VolumeOutcome {
    let (applied_scalar, applied_decibels) = read_applied(hogged, current);
    VolumeOutcome {
        scalar: applied_scalar,
        decibels: applied_decibels,
        previous_scalar: current,
        lowered_by_ceiling: false,
        warning: Some(format!(
            "volume em {:.0}% e não consegui baixá-lo ({error})",
            current * 100.0
        )),
    }
}

/// Apoio aos testes que simulam a perda do device sem hardware.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Sinais ao vivo que o teste controla, contando as leituras.
    #[derive(Clone)]
    pub(crate) struct SondaFalsa {
        sinais: Arc<Mutex<LiveSignals>>,
        leituras: Arc<std::sync::atomic::AtomicUsize>,
        volume: Arc<Mutex<(Option<f32>, Option<bool>)>>,
        leituras_de_volume: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl SondaFalsa {
        pub(crate) fn mudar(&self, sinais: LiveSignals) {
            *self.sinais.lock().expect("sonda") = sinais;
        }

        pub(crate) fn leituras(&self) -> usize {
            self.leituras.load(std::sync::atomic::Ordering::SeqCst)
        }

        pub(crate) fn mudar_volume(&self, escalar: Option<f32>, mudo: Option<bool>) {
            *self.volume.lock().expect("sonda") = (escalar, mudo);
        }

        pub(crate) fn leituras_de_volume(&self) -> usize {
            self.leituras_de_volume
                .load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl DeviceProbe for SondaFalsa {
        fn signals(&self, _device: AudioObjectID, _stream: AudioObjectID) -> LiveSignals {
            self.leituras
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.sinais.lock().expect("sonda")
        }

        fn volume(&self, _device: AudioObjectID) -> (Option<f32>, Option<bool>) {
            self.leituras_de_volume
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.volume.lock().expect("sonda")
        }
    }

    pub(crate) const PID_DE_TESTE: i32 = 4242;

    pub(crate) fn formato_de_teste() -> crate::recovery::PhysicalFormat {
        crate::recovery::PhysicalFormat {
            sample_rate: 44_100.0,
            format_id: coreaudio_sys::kAudioFormatLinearPCM,
            flags: coreaudio_sys::kAudioFormatFlagIsFloat,
            bits_per_channel: 32,
            channels: 2,
            bytes_per_frame: 8,
        }
    }

    pub(crate) fn sinais_ok() -> LiveSignals {
        LiveSignals {
            alive: Some(true),
            hog_owner: Some(PID_DE_TESTE),
            nominal_rate: Some(44_100.0),
            physical: Some(formato_de_teste()),
        }
    }

    pub(crate) fn sinais_de_device_morto() -> LiveSignals {
        LiveSignals {
            alive: Some(false),
            ..sinais_ok()
        }
    }

    /// Um engine que retém um device de mentira, com a sonda e os listeners na mão do teste.
    pub(crate) fn engine_retendo(estado: PlayerState) -> (Engine, SondaFalsa, Arc<WatchState>) {
        engine_retendo_em(estado, &crate::fixtures::path("t44_16.flac"))
    }

    pub(crate) fn engine_retendo_em(
        estado: PlayerState,
        caminho: &str,
    ) -> (Engine, SondaFalsa, Arc<WatchState>) {
        let sonda = SondaFalsa {
            sinais: Arc::new(Mutex::new(sinais_ok())),
            leituras: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            volume: Arc::new(Mutex::new((Some(0.5), Some(false)))),
            leituras_de_volume: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        };
        let engine = Engine::with_probe(Box::new(sonda.clone()));
        let watch = Arc::new(WatchState::new());
        engine.hold_for_test(
            estado,
            crate::recovery::Baseline {
                pid: PID_DE_TESTE,
                nominal_rate: 44_100.0,
                physical: formato_de_teste(),
            },
            Arc::clone(&watch),
            caminho,
        );
        (engine, sonda, watch)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn engine_novo_comeca_em_idle() {
        let engine = Engine::new();
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    fn ler_a_fila_nao_espera_um_comando_que_segura_o_engine() {
        let engine = Engine::new();
        engine.append_tracks(vec!["/tmp/a.flac".to_string()]);

        // Um comando restaurando ou adquirindo o device segura este lock por segundos.
        let comando = engine.lock();
        assert!(engine.try_queue_entries().is_none());
        drop(comando);

        assert_eq!(engine.try_queue_entries().map(|fila| fila.len()), Some(1));
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
    fn play_sem_carregar_e_erro() {
        let engine = Engine::new();
        assert!(engine.play().is_err());
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    fn pause_sem_tocar_e_erro() {
        let engine = Engine::new();
        assert!(engine.pause().is_err());
    }

    #[test]
    fn shutdown_de_engine_parado_e_inofensivo() {
        let engine = Engine::new();
        assert!(engine.shutdown().is_ok());
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    fn append_preserva_ordem_e_incrementa_versao_uma_vez() {
        let engine = Engine::new();
        assert_eq!(
            engine.append_tracks(vec!["b.flac".to_string(), "a.wav".to_string()]),
            2
        );
        let entries = engine.queue_entries();
        let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["b.flac", "a.wav"]);
        assert!(entries.iter().all(|entry| entry.failure.is_none()));
        assert_eq!(engine.status().queue_len(), 2);
        assert_eq!(engine.status().queue_version(), 1);
    }

    #[test]
    fn cada_entrada_ganha_um_id_unico_e_crescente_mesmo_com_caminho_repetido() {
        let engine = Engine::new();
        engine.append_tracks(vec!["a.flac".to_string(), "a.flac".to_string()]);
        engine.append_tracks(vec!["b.flac".to_string()]);

        let ids: Vec<u64> = engine
            .queue_entries()
            .iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "{ids:?}");
    }

    #[test]
    fn limpar_a_fila_nao_reaproveita_ids() {
        let engine = Engine::new();
        engine.append_tracks(vec!["a.flac".to_string()]);
        let antes = engine.queue_entries()[0].id;

        engine.clear_queue_entries();
        engine.append_tracks(vec!["a.flac".to_string()]);

        assert!(engine.queue_entries()[0].id > antes);
    }

    #[test]
    fn o_indice_de_um_id_acompanha_as_remocoes() {
        let engine = Engine::new();
        engine.append_tracks(vec![
            "a.flac".to_string(),
            "b.flac".to_string(),
            "c.flac".to_string(),
        ]);
        let ids: Vec<u64> = engine
            .queue_entries()
            .iter()
            .map(|entry| entry.id)
            .collect();

        engine
            .remove_queue_entry(0)
            .expect("deveria remover a primeira");

        assert_eq!(engine.queue_index_of(ids[0]), None);
        assert_eq!(engine.queue_index_of(ids[2]), Some(1));
    }

    #[test]
    fn remover_fora_do_intervalo_nao_muda_a_fila() {
        let engine = Engine::new();
        engine.append_tracks(vec!["a.flac".to_string()]);
        let before = engine.queue_entries();
        let version = engine.status().queue_version();

        assert!(engine.remove_queue_entry(7).is_err());
        assert_eq!(engine.queue_entries(), before);
        assert_eq!(engine.status().queue_version(), version);
    }

    #[test]
    fn limpar_as_entradas_esvazia_a_fila_e_incrementa_a_versao() {
        let engine = Engine::new();
        engine.append_tracks(vec!["a.flac".to_string()]);
        engine.set_current_index(Some(0));
        let version = engine.status().queue_version();

        engine.clear_queue_entries();

        assert!(engine.queue_entries().is_empty());
        assert_eq!(engine.status().current_index(), None);
        assert_eq!(engine.status().queue_len(), 0);
        assert_eq!(engine.status().queue_version(), version + 1);
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    fn perda_notificada_so_marca_suspeita_no_poll() {
        let (engine, sonda, watch) = engine_retendo(PlayerState::Playing);
        sonda.mudar(sinais_de_device_morto());
        watch.record(crate::device_watch::Watched::Alive);

        engine.poll_finished();

        // O poll roda na main thread: soltar e restaurar o device ali travaria a janela.
        assert!(engine.status().device_suspect());
        assert!(engine.holds_device_for_test());
        assert_eq!(engine.state(), PlayerState::Playing);
    }

    #[test]
    fn sem_notificacao_o_poll_nao_le_o_device() {
        let (engine, sonda, _watch) = engine_retendo(PlayerState::Paused);

        engine.poll_finished();
        engine.poll_finished();
        engine.poll_finished();

        assert_eq!(sonda.leituras(), 0);
    }

    #[test]
    fn notificacao_com_device_igual_ao_da_aquisicao_nao_e_suspeita() {
        // O próprio acquire gera notificações de rate e formato, que podem chegar depois dele.
        let (engine, sonda, watch) = engine_retendo(PlayerState::Playing);
        watch.record(crate::device_watch::Watched::NominalRate);

        engine.poll_finished();

        assert_eq!(sonda.leituras(), 1);
        assert!(!engine.status().device_suspect());
    }

    #[test]
    fn mudanca_de_volume_ou_mudo_nao_faz_o_poll_ler_o_device() {
        // O slider escreve o volume a cada quadro do arrasto; nada disso diz respeito à perda.
        let (engine, sonda, watch) = engine_retendo(PlayerState::Playing);
        watch.record(crate::device_watch::Watched::Volume);
        watch.record(crate::device_watch::Watched::Mute);

        engine.poll_finished();

        assert_eq!(sonda.leituras(), 0);
    }

    #[test]
    fn volume_mudado_por_fora_e_adotado_para_a_proxima_faixa() {
        let (engine, sonda, watch) = engine_retendo(PlayerState::Playing);
        engine.status().set_volume(0.5);
        sonda.mudar_volume(Some(0.25), Some(false));
        watch.record(crate::device_watch::Watched::Volume);

        engine.poll_finished();

        // A faixa seguinte não pode reaplicar os 50% por cima de quem baixou para 25%.
        assert_eq!(engine.status().volume(), Some(0.25));
        assert_eq!(engine.requested_volume_for_test(), Some(0.25));
    }

    #[test]
    fn o_proprio_volume_notificado_nao_vira_pedido() {
        let (engine, sonda, watch) = engine_retendo(PlayerState::Playing);
        engine.status().set_volume(0.5);
        sonda.mudar_volume(Some(0.5), Some(false));
        watch.record(crate::device_watch::Watched::Volume);

        engine.poll_finished();

        assert_eq!(engine.requested_volume_for_test(), None);
    }

    #[test]
    fn mudo_por_fora_aparece_no_status() {
        let (engine, sonda, watch) = engine_retendo(PlayerState::Paused);
        sonda.mudar_volume(Some(0.5), Some(true));
        watch.record(crate::device_watch::Watched::Mute);

        engine.poll_finished();

        assert!(engine.status().muted());
    }

    #[test]
    fn sem_notificacao_de_volume_o_poll_nao_le_o_volume() {
        let (engine, sonda, _watch) = engine_retendo(PlayerState::Playing);
        engine.poll_finished();
        engine.poll_finished();
        assert_eq!(sonda.leituras_de_volume(), 0);
    }

    #[test]
    fn volume_mudado_sem_notificacao_e_lido_antes_de_soltar_o_device() {
        // A notificação pode não ter chegado quando a faixa acaba: a restauração devolveria o
        // volume de antes e a mudança se perderia.
        let (engine, sonda, _watch) = engine_retendo(PlayerState::Finished);
        engine.status().set_volume(0.5);
        sonda.mudar_volume(Some(0.25), Some(false));

        engine.shutdown().expect("shutdown sem hardware");

        assert_eq!(engine.requested_volume_for_test(), Some(0.25));
    }

    #[test]
    fn retomar_com_formato_mudado_e_partida_que_falha_volta_a_loaded() {
        // Sem isto o player ficava pausado sem device, e o Play seguinte só dizia "recarregue".
        let (engine, sonda, _watch) = engine_retendo_em(
            PlayerState::Paused,
            "/tmp/hog-audio-nao-existe-retomar.flac",
        );
        sonda.mudar(LiveSignals {
            nominal_rate: Some(48_000.0),
            ..sinais_ok()
        });

        assert!(engine.play_classified().is_err());

        assert_eq!(engine.state(), PlayerState::Loaded);
        assert_eq!(engine.status().state(), PlayerState::Loaded);
        assert!(!engine.holds_device_for_test());
    }

    #[test]
    fn com_o_device_sob_suspeita_o_volume_so_fica_pedido() {
        // Entre a suspeita e o `recover`, o device pode ser de outro app: escrever nele mexeria
        // no áudio de quem tomou o hog.
        let (engine, _sonda, _watch) = engine_retendo(PlayerState::Playing);
        engine.status().set_device_suspect(true);

        engine
            .set_volume(0.3)
            .expect("o pedido deveria ficar guardado");

        assert_eq!(engine.requested_volume_for_test(), Some(f64::from(0.3f32)));
    }

    #[test]
    fn volume_de_quem_tomou_o_hog_nao_vira_pedido() {
        let (engine, sonda, _watch) = engine_retendo(PlayerState::Playing);
        engine.status().set_volume(0.3);
        sonda.mudar(LiveSignals {
            hog_owner: Some(99),
            ..sinais_ok()
        });
        sonda.mudar_volume(Some(0.9), Some(false));
        engine.status().set_device_suspect(true);

        assert!(engine.recover().is_some());

        assert_eq!(engine.requested_volume_for_test(), None);
    }

    #[test]
    fn recover_confirma_a_perda_e_volta_a_loaded_com_a_faixa() {
        let (engine, sonda, _watch) = engine_retendo(PlayerState::Paused);
        sonda.mudar(sinais_de_device_morto());
        engine.status().set_device_suspect(true);
        engine.status().add_frames(1_000);

        let problema = engine.recover().expect("a perda deveria ser confirmada");

        assert!(problema.contains("desconectado"), "{problema}");
        assert!(!engine.holds_device_for_test());
        assert_eq!(engine.state(), PlayerState::Loaded);
        assert_eq!(engine.status().state(), PlayerState::Loaded);
        assert!(!engine.status().device_suspect());
        assert_eq!(engine.status().problem_seq(), 1);
        assert_eq!(
            engine.status().problem().as_deref(),
            Some(problema.as_str())
        );
        assert_eq!(engine.status().frames_rendered(), 0);
        assert_eq!(
            engine.loaded_path_for_test(),
            Some(crate::fixtures::path("t44_16.flac"))
        );
    }

    #[test]
    fn recover_com_device_bom_so_limpa_a_suspeita() {
        let (engine, _sonda, _watch) = engine_retendo(PlayerState::Playing);
        engine.status().set_device_suspect(true);

        assert_eq!(engine.recover(), None);

        assert!(engine.holds_device_for_test());
        assert_eq!(engine.state(), PlayerState::Playing);
        assert!(!engine.status().device_suspect());
        assert_eq!(engine.status().problem_seq(), 0);
    }

    #[test]
    fn carregar_outra_faixa_com_o_device_perdido_e_erro_de_device() {
        // É o caminho de advance, navigate, select e remove: nenhum deles pode seguir para
        // outro device por conta própria.
        let (engine, sonda, _watch) = engine_retendo(PlayerState::Finished);
        sonda.mudar(sinais_de_device_morto());

        let resultado = engine.load_classified(&crate::fixtures::path("t96_24.flac"));

        assert!(matches!(resultado, Err(LoadFailure::DeviceLost(_))));
        assert!(!engine.holds_device_for_test());
        assert_eq!(engine.state(), PlayerState::Loaded);
        assert_eq!(engine.status().problem_seq(), 1);
    }

    #[test]
    fn frames_parados_tocando_fazem_o_poll_conferir_o_device() {
        let (engine, sonda, _watch) = engine_retendo(PlayerState::Playing);
        engine.poll_finished();
        assert_eq!(sonda.leituras(), 0);

        engine.backdate_watchdog_for_test(std::time::Duration::from_millis(600));
        engine.poll_finished();
        assert_eq!(sonda.leituras(), 1);
        assert!(!engine.status().device_suspect());

        sonda.mudar(sinais_de_device_morto());
        engine.backdate_watchdog_for_test(std::time::Duration::from_millis(600));
        engine.poll_finished();
        assert!(engine.status().device_suspect());
    }

    #[test]
    fn frames_andando_nao_disparam_o_watchdog() {
        let (engine, sonda, _watch) = engine_retendo(PlayerState::Playing);
        engine.backdate_watchdog_for_test(std::time::Duration::from_millis(600));
        engine.status().add_frames(512);

        engine.poll_finished();

        assert_eq!(sonda.leituras(), 0);
    }

    #[test]
    fn finish_with_error_preserva_o_erro_original_quando_nao_ha_nada_a_restaurar() {
        // Não precisa de hardware: um `HoggedDevice` que nunca passou por `acquire()` não tem
        // nada para `finish()` desfazer, e `finish()` devolve `Ok(())`. O que isto prova é que
        // `finish_with_error` não inventa sucesso nem troca o erro original por outra coisa
        // nesse caminho — a rota que um `restore_error` de verdade percorreria só é
        // exercitável com um device real recusando a restauração, fora do alcance de um teste
        // sem hardware.
        let hogged = HoggedDevice::new();
        let erro = finish_with_error(hogged, "erro original".to_string());
        assert_eq!(erro, "erro original");
    }

    #[test]
    #[ignore = "precisa de um device de saída real"]
    fn load_de_flac_valido_vai_para_loaded() {
        let path = crate::fixtures::path("t96_24.flac");
        let engine = Engine::new();
        let track = engine.load(&path).expect("deveria carregar");

        assert_eq!(engine.state(), PlayerState::Loaded);
        assert_eq!(track.sample_rate, 96000.0);
        assert_eq!(track.bit_depth, Some(24));
        assert_eq!(track.bitrate_kbps, None);
        assert_eq!(track.channels, 2);
        assert_eq!(track.codec, "flac");
        assert!(!track.device_name.is_empty());
        assert!((track.total_seconds - 6.0).abs() < 0.1);
        assert!((engine.status().total_seconds() - 6.0).abs() < 0.1);
    }

    #[test]
    #[ignore = "precisa de um device de saída real"]
    fn load_de_aac_negocia_float32_no_rate_do_arquivo() {
        let engine = Engine::new();
        let track = engine
            .load(&crate::fixtures::path("t44_aac.m4a"))
            .expect("deveria carregar o AAC");

        assert_eq!(engine.state(), PlayerState::Loaded);
        assert_eq!(track.bit_depth, None);
        assert!(track.bitrate_kbps.is_some());
        let report = engine.device_report().expect("deveria ter negociado");
        assert_eq!(report.negotiated, "44100 Hz / 32 bits float");
    }

    #[test]
    #[ignore = "precisa de um device de saída real"]
    fn carregar_de_novo_substitui_a_faixa_anterior() {
        let a = crate::fixtures::path("t96_24.flac");
        let b = crate::fixtures::path("t44_16.flac");
        let engine = Engine::new();
        engine.load(&a).expect("deveria carregar o primeiro");
        let track = engine.load(&b).expect("deveria carregar o segundo");
        assert_eq!(track.sample_rate, 44100.0);
        assert_eq!(engine.state(), PlayerState::Loaded);
    }

    #[test]
    #[ignore = "precisa de um device de saída real"]
    fn negociacao_recusada_preserva_a_faixa_carregada() {
        // 192 kHz não está entre os rates do device built-in: a negociação recusa antes de
        // qualquer teardown acontecer.
        let anterior = crate::fixtures::path("t96_24.flac");
        let recusado = crate::fixtures::path("t192_24.flac");
        let engine = Engine::new();
        engine
            .load(&anterior)
            .expect("deveria carregar a primeira faixa");

        assert!(engine.load(&recusado).is_err());

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

    #[test]
    #[ignore = "precisa de um device de saída real; toma o device por alguns segundos"]
    fn carregar_segunda_faixa_com_device_retido_nao_troca_de_hardware() {
        // Sob hog mode, o macOS aponta "o device de saída padrão" para outro hardware — o
        // mesmo efeito documentado no README (seção Volume) e que fez o teste de mutação da
        // Correção 1 desta onda ter que sair do nível de `Engine`. Sem a correção que reusa
        // `inner.device` quando `inner.hogged.is_some()`, carregar uma segunda faixa com a
        // primeira ainda tocando faria a negociação (e a aquisição seguinte) mirar outro
        // device físico — o som mudaria de saída sem o usuário pedir. Neste Mac, com fone
        // conectado, isso é observável de forma direta: o id/nome do device muda de "Fones de
        // Ouvido Externos" para "Alto-falantes (MacBook Air)" quando `load()` volta a
        // consultar o default enquanto o fone está retido.
        let primeira = crate::fixtures::path("t96_24.flac");
        let segunda = crate::fixtures::path("t44_16.flac");

        let engine = Engine::new();
        engine
            .load(&primeira)
            .expect("deveria carregar a primeira faixa");
        engine.play().expect("deveria tocar a primeira faixa");
        assert_eq!(engine.state(), PlayerState::Playing);

        let (id_antes, nome_antes) = {
            let inner = engine.lock();
            let d = inner
                .device
                .as_ref()
                .expect("device deveria estar consultado");
            (d.id, d.name.clone())
        };

        // Carrega a segunda faixa com a primeira ainda tocando/retida: é exatamente a
        // condição em que o "device de saída padrão" do sistema mente.
        engine
            .load(&segunda)
            .expect("deveria carregar a segunda faixa");
        assert_eq!(engine.state(), PlayerState::Loaded);

        let (id_depois, nome_depois) = {
            let inner = engine.lock();
            let d = inner
                .device
                .as_ref()
                .expect("device deveria estar consultado");
            (d.id, d.name.clone())
        };

        assert_eq!(
            id_antes, id_depois,
            "load() deveria reutilizar o device retido ({nome_antes}, id {id_antes}), mas \
             passou a usar outro ({nome_depois}, id {id_depois}) — sinal de que voltou a \
             consultar o default output device com a primeira faixa ainda retida"
        );
        assert_eq!(
            nome_antes, nome_depois,
            "o nome do device também trocou — mesmo sintoma, checagem independente do id"
        );

        engine.shutdown().expect("deveria restaurar o device");
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    #[ignore = "precisa de um device de saída real; toma o device por alguns segundos"]
    fn ciclo_play_pause_play_shutdown() {
        let path = crate::fixtures::path("t96_24.flac");
        let engine = Engine::new();
        engine.load(&path).expect("deveria carregar");

        engine.play().expect("deveria tocar");
        assert_eq!(engine.state(), PlayerState::Playing);
        std::thread::sleep(std::time::Duration::from_millis(400));
        let decorrido = engine.status().elapsed_seconds();
        assert!(decorrido > 0.0, "o tempo decorrido deveria avançar tocando");

        engine.pause().expect("deveria pausar");
        assert_eq!(engine.state(), PlayerState::Paused);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let parado = engine.status().elapsed_seconds();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(
            (engine.status().elapsed_seconds() - parado).abs() < 1e-6,
            "o tempo não pode avançar com o player pausado"
        );

        engine.play().expect("deveria retomar");
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(
            engine.status().elapsed_seconds() > parado,
            "o tempo deveria voltar a avançar depois de retomar"
        );

        engine.shutdown().expect("deveria restaurar o device");
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    #[ignore = "precisa de um device de saída real; toma o device por alguns segundos"]
    fn tocar_de_novo_apos_o_fim_nao_duplica_a_posse_do_device() {
        // Verificação estrutural, não de fumaça: sob o defeito antigo, o Drop do
        // HoggedDevice velho reverte hog mode e sample rate por baixo da sessão nova, e toda
        // chamada envolvida retorna noErr — estado do engine (Playing, tempo avançando,
        // shutdown sem erro) continuaria parecendo saudável mesmo corrompido. Só a leitura
        // direta do hardware denuncia; por isso as duas asserções finais leem
        // kAudioDevicePropertyHogMode e kAudioDevicePropertyNominalSampleRate no meio da
        // segunda reprodução, antes do shutdown (que solta o device de propósito e apagaria o
        // sintoma).
        use crate::ffi::{get_property, property_address};
        use coreaudio_sys::{
            kAudioDevicePropertyHogMode, kAudioDevicePropertyNominalSampleRate,
            kAudioObjectPropertyScopeGlobal, pid_t,
        };

        let path = crate::fixtures::path("t44_16.flac");
        let engine = Engine::new();
        engine.load(&path).expect("deveria carregar");

        engine.play().expect("deveria tocar a primeira vez");
        assert_eq!(engine.state(), PlayerState::Playing);

        // Força o fim sem esperar a faixa inteira: é a mesma flag que o IOProc real marca
        // (`playback.rs`, quando a produtora terminou e o ring esvaziou), então
        // `poll_finished` segue exatamente o caminho de um fim de faixa de verdade — inclusive
        // deixando `inner.hogged`/`playback`/`producer` retidos, que é o estado que expõe o
        // bug: um segundo `play()` aqui criava um `HoggedDevice` novo sobre o antigo ainda
        // vivo.
        {
            let inner = engine.lock();
            inner
                .playback
                .as_ref()
                .expect("deveria ter playback depois de tocar")
                .finished
                .store(true, std::sync::atomic::Ordering::Release);
        }
        engine.poll_finished();
        assert_eq!(engine.state(), PlayerState::Finished);

        engine
            .play()
            .expect("a segunda reprodução deveria funcionar depois do fim da primeira");
        assert_eq!(engine.state(), PlayerState::Playing);
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert!(
            engine.status().elapsed_seconds() > 0.0,
            "o tempo decorrido deveria avançar na segunda reprodução"
        );

        let device_id = engine
            .lock()
            .device
            .as_ref()
            .expect("device deveria estar consultado")
            .id;

        // Fato 1: o dono do hog mode é este processo. Se o Drop do HoggedDevice antigo
        // tivesse liberado o hog por baixo da sessão nova, isto leria outro dono (ou nenhum).
        let hog_address =
            property_address(kAudioDevicePropertyHogMode, kAudioObjectPropertyScopeGlobal);
        let owner: pid_t =
            get_property(device_id, &hog_address).expect("deveria ler o dono do hog mode");
        assert_eq!(
            owner,
            std::process::id() as pid_t,
            "o hog mode deveria pertencer a este processo depois da segunda reprodução; um              dono diferente denuncia o Drop do HoggedDevice antigo liberando por baixo da              sessão nova"
        );

        // Fato 2: o sample rate do device é o da faixa (44100 Hz). Se o Drop antigo tivesse
        // revertido o rate, isto leria o valor pré-hog em vez do negociado.
        let rate_address = property_address(
            kAudioDevicePropertyNominalSampleRate,
            kAudioObjectPropertyScopeGlobal,
        );
        let rate: f64 =
            get_property(device_id, &rate_address).expect("deveria ler o sample rate nominal");
        assert!(
            (rate - 44100.0).abs() < 0.5,
            "o sample rate deveria continuar em 44100 Hz (o negociado) durante a segunda              reprodução; {rate} Hz denunciaria o Drop do HoggedDevice antigo revertendo para              a taxa pré-hog"
        );

        engine
            .shutdown()
            .expect("deveria restaurar o device depois da segunda reprodução");
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    #[ignore = "precisa de um device de saída real; toma o device por alguns segundos e mexe no volume"]
    fn volume_baixado_durante_a_reproducao_sobrevive_ao_fim_da_faixa() {
        // Reproduz o bug: o usuário baixa o volume enquanto o device está retido (`hogged` é
        // `Some`), a faixa termina, e o player toca de novo. Sem persistir o pedido em
        // `inner.volume` nesse ramo, `stop_and_release` restaura o volume pré-hog e a
        // aquisição seguinte aplica só o teto sobre essa leitura — o volume escolhido pelo
        // usuário some, e pode até subir.
        let path = crate::fixtures::path("t96_24.flac");

        let engine = Engine::new();
        engine.load(&path).expect("deveria carregar");
        engine.play().expect("deveria tocar");
        assert_eq!(engine.state(), PlayerState::Playing);

        // Bem abaixo do teto padrão (50%): se o pedido não persistir, a aquisição seguinte
        // aplicaria o teto sobre a leitura pré-hog, que fica livre para ficar bem acima disto.
        engine
            .set_volume(0.05)
            .expect("deveria conseguir baixar o volume com o device na mão");
        assert!(
            matches!(
                engine.lock().volume,
                Some(ref request) if (request.value - 0.05).abs() < 1e-9
            ),
            "o pedido de volume deveria ficar guardado em inner.volume mesmo com o device retido"
        );

        // Força o fim da faixa do jeito que o IOProc real marcaria — mesma técnica do teste
        // `tocar_de_novo_apos_o_fim_nao_duplica_a_posse_do_device` logo acima.
        {
            let inner = engine.lock();
            inner
                .playback
                .as_ref()
                .expect("deveria ter playback depois de tocar")
                .finished
                .store(true, std::sync::atomic::Ordering::Release);
        }
        engine.poll_finished();
        assert_eq!(engine.state(), PlayerState::Finished);

        engine
            .play()
            .expect("a segunda reprodução deveria funcionar depois do fim da primeira");
        assert_eq!(engine.state(), PlayerState::Playing);

        let outcome = engine
            .volume_outcome()
            .expect("deveria ter um resultado de volume depois da segunda aquisição");
        assert!(
            (outcome.scalar - 0.05).abs() < 0.02,
            "o volume da segunda reprodução deveria continuar em torno de 5% (o pedido do \
             usuário), está em {:.0}% — sinal de que o pedido foi descartado e o teto foi \
             aplicado por cima da leitura pré-hog",
            outcome.scalar * 100.0
        );

        engine
            .shutdown()
            .expect("deveria restaurar o device depois da segunda reprodução");
        assert_eq!(engine.state(), PlayerState::Idle);
    }
}

#[cfg(test)]
mod tuning_tests {
    use super::*;
    use crate::tuning::curve::CurvePoint;

    fn client_float32(channels: u32) -> AudioStreamBasicDescription {
        AudioStreamBasicDescription {
            mSampleRate: 48000.0,
            mFormatID: coreaudio_sys::kAudioFormatLinearPCM,
            mFormatFlags: coreaudio_sys::kAudioFormatFlagIsFloat
                | coreaudio_sys::kAudioFormatFlagIsPacked,
            mBytesPerPacket: 4 * channels,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4 * channels,
            mChannelsPerFrame: channels,
            mBitsPerChannel: 32,
            mReserved: 0,
        }
    }

    fn client_int24_em_32(channels: u32) -> AudioStreamBasicDescription {
        let mut client = client_float32(channels);
        client.mFormatFlags = coreaudio_sys::kAudioFormatFlagIsSignedInteger;
        client.mBitsPerChannel = 24;
        client
    }

    fn curva(points: &[(f64, f64)]) -> Curve {
        let points = points
            .iter()
            .map(|&(hz, db)| CurvePoint { hz, db })
            .collect();
        Curve::from_points(points).expect("curva do teste")
    }

    /// A entrega vira float32 quando a afinação está ligada, mantendo taxa e canais. Se voltasse
    /// o formato do device, o filtro receberia inteiro empacotado e leria lixo como amostra.
    #[test]
    fn a_entrega_afinada_e_float32_com_os_canais_do_device() {
        let delivery = float_delivery_for(&client_int24_em_32(2));

        assert_eq!(delivery.mBitsPerChannel, 32);
        assert_eq!(delivery.mBytesPerFrame, 8);
        assert_eq!(delivery.mChannelsPerFrame, 2);
        assert_eq!(delivery.mSampleRate, 48000.0);
        assert!(delivery.mFormatFlags & coreaudio_sys::kAudioFormatFlagIsFloat != 0);
    }

    /// O container por canal vem de `mBytesPerFrame / canais`, nunca de `bits / 8`: 24 bits em
    /// container de 32 é o caso comum, e deduzir empacotamento desloca cada amostra.
    #[test]
    fn o_formato_de_saida_sai_do_que_o_device_publica() {
        let format = output_format_for(&client_int24_em_32(2));

        assert_eq!(format.sample_type, crate::format::SampleType::Integer);
        assert_eq!(format.bits_per_channel, 24);
        assert_eq!(format.bytes_per_sample, 4);

        let format = output_format_for(&client_float32(2));

        assert_eq!(format.sample_type, crate::format::SampleType::Float);
        assert_eq!(format.bytes_per_sample, 4);
    }

    /// Uma curva com ganho positivo tem de ser normalizada antes de virar filtro. Sem isso, o
    /// grave levantado em 12 dB estoura o fundo de escala e o que chega ao DAC é distorção —
    /// e nada no caminho reclamaria, porque saturar não é erro, é só som ruim.
    #[test]
    fn a_curva_e_normalizada_antes_de_virar_filtro() {
        let client = client_float32(1);
        let (mut processor, _) = build_tuning(&curva(&[(20.0, 12.0), (20000.0, 0.0)]), &client)
            .expect("deveria montar a afinação");

        // Senoide de 20 Hz, onde a curva pede os 12 dB, com folga de escala. Sem normalizar, o
        // ganho de 4x levaria isto a 2,0 — o dobro do que cabe.
        let mut samples: Vec<f32> = (0..4096)
            .map(|n| {
                let fase = 2.0 * std::f64::consts::PI * 20.0 * n as f64 / 48000.0;
                0.5 * fase.sin() as f32
            })
            .collect();
        processor.process(&mut samples).expect("deveria filtrar");

        assert_eq!(processor.clipped(), 0, "a curva saturou o fundo de escala");
        let pico = samples.iter().fold(0.0f32, |max, &v| max.max(v.abs()));
        assert!(pico <= 0.55, "pico de {pico} acima do sinal de entrada");
    }

    #[test]
    fn curva_impossivel_de_realizar_e_recusada_na_montagem() {
        let mut client = client_float32(2);
        client.mBitsPerChannel = 20; // fora do byte: o quantizador não tem o que fazer
        client.mFormatFlags = coreaudio_sys::kAudioFormatFlagIsSignedInteger;

        assert!(build_tuning(&curva(&[(20.0, 0.0), (20000.0, 0.0)]), &client).is_err());
    }
}
