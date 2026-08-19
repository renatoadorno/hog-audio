//! A casca imperativa da máquina de estados: aqui moram os efeitos sobre o hardware, o
//! arquivo e as threads. As regras de qual transição é permitida ficam em `transitions`,
//! puras e testáveis sem nada disso.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::thread::JoinHandle;

use coreaudio_sys::AudioStreamBasicDescription;

use crate::device::{query_default_output_device, HoggedDevice, OutputDevice};
use crate::format::{self, Decision, SampleRateRange};
use crate::playback::Playback;
use crate::source::AudioSource;
use crate::status::SharedStatus;
use crate::transitions::{next_state, Command, PlayerState};
use crate::volume::{apply_ceiling, VolumeRequest, VolumeUnit};

/// O mesmo padrão da CLI: sem `--volume` explícito, o device é baixado para no máximo isto.
pub const DEFAULT_CEILING: f64 = 0.5;

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

/// Origem de uma falha de `play`: transição de estado inválida (o usuário precisa mudar o que
/// pediu — recarregar, por exemplo) ou falha ao tomar/reconfigurar o hardware. A CLI não
/// precisa da distinção (`play` achata as duas em `String`), mas a fronteira uniffi ramifica
/// sobre isso — daí `pub(crate)` em vez de inflar a API pública do `Engine` com um tipo de
/// erro novo só para quem está do outro lado do FFI.
pub(crate) enum PlayFailure {
    State(String),
    Device(String),
}

impl PlayFailure {
    fn into_message(self) -> String {
        match self {
            PlayFailure::State(message) | PlayFailure::Device(message) => message,
        }
    }
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
    client: Option<AudioStreamBasicDescription>,
    hogged: Option<HoggedDevice>,
    playback: Option<Arc<Playback>>,
    producer: Option<JoinHandle<()>>,
    stop_producer: Arc<AtomicBool>,
    volume: Option<VolumeRequest>,
    ceiling: f64,
    volume_outcome: Option<VolumeOutcome>,
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
                client: None,
                hogged: None,
                playback: None,
                producer: None,
                stop_producer: Arc::new(AtomicBool::new(false)),
                volume: None,
                ceiling: DEFAULT_CEILING,
                volume_outcome: None,
            }),
        }
    }

    /// Um pânico dentro de um comando não pode deixar o player inutilizável para sempre: o
    /// estado interno é recuperado em vez de propagar o envenenamento do mutex.
    fn lock(&self) -> MutexGuard<'_, EngineInner> {
        self.inner.lock().unwrap_or_else(|poison| poison.into_inner())
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
    pub fn set_requested_volume(&self, volume: Option<VolumeRequest>, ceiling: f64) {
        let mut inner = self.lock();
        inner.volume = volume;
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
        // Abrir o arquivo primeiro, antes de descartar o que estava carregado: se o caminho
        // for inválido, o player continua exatamente como estava.
        let source = AudioSource::open(path)?;
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
        let held_device = {
            let inner = self.lock();
            if inner.hogged.is_some() {
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
            None => query_default_output_device()?,
        };
        let decision = format::negotiate(&file_format, &device.caps);
        if !decision.play {
            return Err(decision.reason);
        }

        let mut inner = self.lock();
        // `teardown` só limpa os campos da faixa carregada; soltar o device é
        // responsabilidade só de `stop_and_release`, chamada aqui explicitamente por quem
        // precisa dela (o erro é descartado pelo mesmo motivo de sempre: uma restauração que
        // falhou não pode impedir a carga de uma faixa nova).
        let _ = self.stop_and_release(&mut inner);
        self.teardown(&mut inner);

        let device_name = device.name.clone();
        // Hoje inalcançável: `next_state(_, Load)` é catch-all `Ok` (ver `transitions.rs`), e
        // este `?` nunca dispara. Mas se `Load` um dia puder ser recusado nalgum estado, ele
        // dispararia *depois* de `teardown` já ter apagado a faixa anterior — exatamente o bug
        // que o comentário acima de `stop_and_release`/`teardown` existe para evitar. Quem
        // tornar `Load` falível precisa mover esta checagem para antes do teardown.
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
            PlayerState::Paused => {
                // O device já deveria estar retido neste estado; sem ele, o problema não é o
                // hardware ter recusado algo, é o player estar num estado que ele mesmo não
                // sabe mais resolver — por isso `State`, não `Device`.
                let hogged = inner.hogged.as_mut().ok_or_else(|| {
                    PlayFailure::State("pausado sem device; recarregue a faixa".to_string())
                })?;
                hogged.resume().map_err(PlayFailure::Device)?;
            }
            PlayerState::Loaded | PlayerState::Finished => self
                .start_common(&mut inner, true)
                .map_err(PlayFailure::Device)?,
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
        match inner.hogged.as_mut() {
            Some(hogged) => {
                hogged.set_volume(scalar)?;
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
        if inner.state != PlayerState::Playing {
            return;
        }
        let terminou = inner
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
    }

    pub fn shutdown(&self) -> Result<(), String> {
        let mut inner = self.lock();
        let restore = self.stop_and_release(&mut inner);
        self.teardown(&mut inner);
        inner.state = PlayerState::Idle;
        self.status.set_state(PlayerState::Idle);
        restore
    }

    fn start_common(&self, inner: &mut EngineInner, attach_io_proc: bool) -> Result<(), String> {
        // Incondicional, mesmo vindo de Loaded (onde nada está retido — vira no-op). O match
        // de `play()` só chega aqui a partir de Loaded ou Finished; vindo de Finished um
        // HoggedDevice antigo pode continuar registrado (o fim da faixa só chama
        // `hogged.stop()`, não solta o device) — criar um segundo aqui sobreporia
        // hog/rate/formato por cima do que a acquire() nova ainda vai gravar como "original",
        // e o Drop do antigo revogaria hog mode e reverteria rate/formato por baixo de uma
        // sessão que se acredita exclusiva — tudo retornando noErr, sem nenhum sinal de erro.
        let _ = self.stop_and_release(inner);

        // Recarrega do início: vindo de Finished o decodificador já se esgotou, e mesmo
        // vindo de Loaded é preciso um AudioSource que possa ser movido para a produtora.
        let path = inner
            .path
            .clone()
            .ok_or_else(|| "nenhum arquivo carregado".to_string())?;
        let mut source = AudioSource::open(&path)?;
        let file_format = source.format();

        // Os dados que o resto da função precisa são copiados aqui para que os empréstimos
        // de `inner` terminem antes das atribuições no fim: sem isso o verificador de
        // empréstimos recusa a função.
        let device = inner
            .device
            .as_ref()
            .ok_or_else(|| "device não consultado".to_string())?;
        let decision = inner
            .decision
            .as_ref()
            .ok_or_else(|| "formato não negociado".to_string())?;

        let mut hogged = HoggedDevice::new();
        let physical = device.physical_formats[decision.physical_format_index as usize];
        hogged.acquire(device, decision.sample_rate, &physical)?;

        // Dali em diante, nenhum erro pode propagar com um `?` solto: `hogged` ainda é local,
        // não movida para `inner.hogged`, e um `?` a derrubaria — o `Drop::restore()` rodaria,
        // mas `restore_error` seria descartado junto, e como `inner.hogged` continuaria `None`
        // um `shutdown()` posterior cairia no ramo `None => Ok(())` e mentiria sucesso mesmo
        // com a restauração tendo falhado. `finish_with_error` fecha essa fresta: finaliza
        // `hogged` explicitamente e concatena os dois erros, para nenhum se perder.

        // Com o device já nosso e antes de qualquer amostra sair: é o único ponto em que dá
        // para garantir que o fone não receba o volume anterior. A publicação no
        // `SharedStatus` fica para o fim da função — ver o comentário perto de
        // `self.status.set_volume` mais abaixo.
        let outcome = match apply_volume(&mut hogged, device, inner.volume.as_ref(), inner.ceiling)
        {
            Ok(outcome) => outcome,
            Err(error) => return Err(finish_with_error(hogged, error)),
        };

        let stream = hogged.stream_format();
        if stream.mFormatID != coreaudio_sys::kAudioFormatLinearPCM {
            return Err(finish_with_error(
                hogged,
                "o device não está em PCM linear; não vou alimentá-lo".to_string(),
            ));
        }

        let (client, non_interleaved) = crate::engine::client_format_for(&stream);
        let check = crate::format::validate_interleaved_format(
            client.mBitsPerChannel,
            client.mBytesPerFrame,
            client.mChannelsPerFrame,
        );
        if !check.ok {
            return Err(finish_with_error(
                hogged,
                format!("formato de entrega inconsistente: {}", check.reason),
            ));
        }

        if let Err(error) = source.set_client_format(&client) {
            return Err(finish_with_error(hogged, error));
        }

        // O decodificador pode ajustar o que aceitou. Divergência aqui é a diferença entre
        // silêncio e ruído em volume total.
        let effective = match source.effective_client_format() {
            Ok(effective) => effective,
            Err(error) => return Err(finish_with_error(hogged, error)),
        };
        if let Err(error) = crate::engine::assert_same_delivery(&effective, &client) {
            return Err(finish_with_error(hogged, error));
        }

        let ring_bytes = client.mSampleRate as usize * client.mBytesPerFrame as usize * 2;
        self.status.set_track(source.total_frames(), file_format.sample_rate);
        let playback = Arc::new(Playback::new(
            ring_bytes,
            &client,
            non_interleaved,
            Arc::clone(&self.status),
        ));

        let stop = Arc::new(AtomicBool::new(false));
        let producer = {
            let playback = Arc::clone(&playback);
            let stop = Arc::clone(&stop);
            let bytes_per_frame = client.mBytesPerFrame as usize;
            std::thread::spawn(move || {
                let mut chunk = vec![0u8; 64 * 1024];
                let frames_per_chunk = (chunk.len() / bytes_per_frame) as u32;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let got = source.read(&mut chunk, frames_per_chunk);
                    if got == 0 {
                        break;
                    }
                    let bytes = got as usize * bytes_per_frame;
                    let mut written = 0usize;
                    while written < bytes && !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        written += playback.ring.write(&chunk[written..bytes]);
                        if written < bytes {
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                    }
                }
                playback
                    .producer_done
                    .store(true, std::sync::atomic::Ordering::Release);
            })
        };

        // Deixa o buffer encher antes de abrir o fluxo, para o começo não sair picotado.
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

        // O modo dump usa exatamente este mesmo caminho, sem só esta última etapa: assim ele
        // percorre decodificador, ring buffer e alinhamento de frame, que é onde estiveram os
        // bugs mais caros. Um atalho que apenas decodificasse não provaria nada disso.
        if attach_io_proc {
            // SAFETY: o io_proc desreferencia este ponteiro cru, sem contagem de referência, a
            // cada callback de tempo real do Core Audio. O invariante que sustenta isso: ao
            // menos um `Arc<Playback>` fica vivo enquanto o callback puder disparar.
            // `stop_and_release` garante a ordem inversa na saída — chama `hogged.stop()`
            // (nenhum callback novo depois disso) e só então junta a produtora e zera
            // `inner.playback`, então o ponteiro nunca é lido depois do `Arc` cair.
            let context = Arc::as_ptr(&playback) as *mut std::ffi::c_void;
            if let Err(error) = hogged.start(Some(crate::playback::io_proc), context) {
                // A produtora e o pré-buffer já rodaram; sem isto, ninguém mais teria a flag
                // `stop` para sinalizar parada, e a thread giraria para sempre enchendo um
                // ring buffer que nenhum consumidor vai esvaziar.
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = producer.join();
                return Err(finish_with_error(hogged, error));
            }
        }

        // Só agora nada mais nesta função pode falhar e derrubar o `HoggedDevice` local (cujo
        // `Drop` reverteria o volume no hardware) — publicar antes deixaria o status
        // mostrando um volume que o device já não tem mais caso alguma checagem acima falhe.
        self.status.set_volume(outcome.scalar);
        inner.volume_outcome = Some(outcome);
        inner.client = Some(client);
        inner.hogged = Some(hogged);
        inner.playback = Some(playback);
        inner.producer = Some(producer);
        inner.stop_producer = stop;
        Ok(())
    }

    /// Prepara tudo como o `play`, menos o IOProc, e devolve o que o consumidor de disco
    /// precisa. Usado pelo `--dump`.
    pub fn start_offline(&self) -> Result<(Arc<Playback>, AudioStreamBasicDescription), String> {
        let mut inner = self.lock();
        let target = next_state(inner.state, Command::Play).map_err(|e| e.message.to_string())?;
        self.start_common(&mut inner, false)?;
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
        if let Some(hogged) = inner.hogged.as_mut() {
            hogged.stop();
        }
        inner
            .stop_producer
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(producer) = inner.producer.take() {
            let _ = producer.join();
        }
        inner.playback = None;
        let restore = match inner.hogged.as_mut() {
            Some(hogged) => hogged.finish(),
            None => Ok(()),
        };
        inner.hogged = None;
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

/// Um pedido explícito é uma garantia: se não der para cumprir, é melhor não tocar do que
/// tocar mais alto do que se pediu. Já o teto é uma rede de proteção — não havendo controle
/// de volume, ou falhando a escrita, avisa e segue, que é o comportamento de sempre.
pub(crate) fn apply_volume(
    hogged: &mut HoggedDevice,
    device: &OutputDevice,
    request: Option<&VolumeRequest>,
    ceiling: f64,
) -> Result<VolumeOutcome, String> {
    if let Some(request) = request {
        let scalar = match request.unit {
            VolumeUnit::Percent => request.value as f32,
            VolumeUnit::Decibels => hogged
                .decibels_to_scalar(request.value)
                .ok_or("este device não converte decibéis; use porcentagem")?,
        };
        let before = device.volume;
        hogged.set_volume(scalar)?;
        let (applied_scalar, applied_decibels) = read_applied(hogged, scalar);
        return Ok(VolumeOutcome {
            scalar: applied_scalar,
            decibels: applied_decibels,
            previous_scalar: before,
            lowered_by_ceiling: false,
            warning: None,
        });
    }

    if device.volume < 0.0 {
        return Ok(VolumeOutcome {
            scalar: -1.0,
            decibels: f64::NAN,
            previous_scalar: device.volume,
            lowered_by_ceiling: false,
            warning: None,
        });
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
        return Ok(VolumeOutcome {
            scalar: applied_scalar,
            decibels: applied_decibels,
            previous_scalar: current,
            lowered_by_ceiling: false,
            warning: None,
        });
    }

    if let Err(error) = hogged.set_volume(decision.scalar as f32) {
        // O teto é rede de proteção, não garantia: não conseguir baixar não pode abortar a
        // reprodução, ao contrário de um pedido explícito falhar. O texto do erro vai no
        // aviso para não ficar indistinguível de "já estava abaixo do teto".
        let (applied_scalar, applied_decibels) = read_applied(hogged, current);
        return Ok(VolumeOutcome {
            scalar: applied_scalar,
            decibels: applied_decibels,
            previous_scalar: current,
            lowered_by_ceiling: false,
            warning: Some(format!(
                "volume em {:.0}% e não consegui baixá-lo ({error})",
                current * 100.0
            )),
        });
    }

    let (applied_scalar, applied_decibels) = read_applied(hogged, decision.scalar as f32);
    Ok(VolumeOutcome {
        scalar: applied_scalar,
        decibels: applied_decibels,
        previous_scalar: current,
        lowered_by_ceiling: true,
        warning: None,
    })
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
        let path = "../testdata/t96_24.flac";
        if !std::path::Path::new(path).exists() {
            panic!("fixture ausente: {path} (gere com ffmpeg)");
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
            panic!("fixture ausente: {a} ou {b} (gere com ffmpeg)");
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
            panic!("fixture ausente: {anterior} ou {recusado} (gere com ffmpeg)");
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
        let primeira = "../testdata/t96_24.flac";
        let segunda = "../testdata/t44_16.flac";
        for path in [primeira, segunda] {
            if !std::path::Path::new(path).exists() {
                panic!("fixture ausente: {path} (gere com ffmpeg)");
            }
        }

        let engine = Engine::new();
        engine.load(primeira).expect("deveria carregar a primeira faixa");
        engine.play().expect("deveria tocar a primeira faixa");
        assert_eq!(engine.state(), PlayerState::Playing);

        let (id_antes, nome_antes) = {
            let inner = engine.lock();
            let d = inner.device.as_ref().expect("device deveria estar consultado");
            (d.id, d.name.clone())
        };

        // Carrega a segunda faixa com a primeira ainda tocando/retida: é exatamente a
        // condição em que o "device de saída padrão" do sistema mente.
        engine.load(segunda).expect("deveria carregar a segunda faixa");
        assert_eq!(engine.state(), PlayerState::Loaded);

        let (id_depois, nome_depois) = {
            let inner = engine.lock();
            let d = inner.device.as_ref().expect("device deveria estar consultado");
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
        let path = "../testdata/t96_24.flac";
        if !std::path::Path::new(path).exists() {
            panic!("fixture ausente: {path} (gere com ffmpeg)");
        }
        let engine = Engine::new();
        engine.load(path).expect("deveria carregar");

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

        let path = "../testdata/t44_16.flac";
        if !std::path::Path::new(path).exists() {
            panic!("fixture ausente: {path} (gere com ffmpeg)");
        }
        let engine = Engine::new();
        engine.load(path).expect("deveria carregar");

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
        let hog_address = property_address(kAudioDevicePropertyHogMode, kAudioObjectPropertyScopeGlobal);
        let owner: pid_t =
            get_property(device_id, &hog_address).expect("deveria ler o dono do hog mode");
        assert_eq!(
            owner,
            std::process::id() as pid_t,
            "o hog mode deveria pertencer a este processo depois da segunda reprodução; um              dono diferente denuncia o Drop do HoggedDevice antigo liberando por baixo da              sessão nova"
        );

        // Fato 2: o sample rate do device é o da faixa (44100 Hz). Se o Drop antigo tivesse
        // revertido o rate, isto leria o valor pré-hog em vez do negociado.
        let rate_address =
            property_address(kAudioDevicePropertyNominalSampleRate, kAudioObjectPropertyScopeGlobal);
        let rate: f64 =
            get_property(device_id, &rate_address).expect("deveria ler o sample rate nominal");
        assert!(
            (rate - 44100.0).abs() < 0.5,
            "o sample rate deveria continuar em 44100 Hz (o negociado) durante a segunda              reprodução; {rate} Hz denunciaria o Drop do HoggedDevice antigo revertendo para              a taxa pré-hog"
        );

        engine.shutdown().expect("deveria restaurar o device depois da segunda reprodução");
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
        let path = "../testdata/t96_24.flac";
        if !std::path::Path::new(path).exists() {
            panic!("fixture ausente: {path} (gere com ffmpeg)");
        }

        let engine = Engine::new();
        engine.load(path).expect("deveria carregar");
        engine.play().expect("deveria tocar");
        assert_eq!(engine.state(), PlayerState::Playing);

        // Bem abaixo do teto padrão (50%): se o pedido não persistir, a aquisição seguinte
        // aplicaria o teto sobre a leitura pré-hog, que fica livre para ficar bem acima disto.
        engine.set_volume(0.05).expect("deveria conseguir baixar o volume com o device na mão");
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

        engine.shutdown().expect("deveria restaurar o device depois da segunda reprodução");
        assert_eq!(engine.state(), PlayerState::Idle);
    }
}
