//! Camada HAL: consulta do device, modo exclusivo, lock de rate e formato, volume, IOProc e
//! restauração do estado original.

use coreaudio_sys::*;

use crate::ffi::{
    get_property, get_property_array, get_string_property, has_property, is_property_settable,
    os_status_text, property_address, set_property,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::device_watch::{WatchState, Watched};
use crate::format::{DeviceCaps, PhysicalFormatDesc, SampleRateRange, SampleType};
use crate::recovery::{Baseline, LiveSignals, PhysicalFormat};

const SCOPE_GLOBAL: u32 = kAudioObjectPropertyScopeGlobal;
const SCOPE_OUTPUT: u32 = kAudioObjectPropertyScopeOutput;

/// O device de saída e tudo que ele aceita. `physical_formats` é paralelo a
/// `caps.physical_formats`: o núcleo escolhe um índice, aqui está o descritor correspondente.
///
/// `Clone` existe para `Engine::load` poder reutilizar o device já retido em vez de perguntar
/// ao sistema de novo — sob hog mode, a resposta do sistema é outro hardware (ver o
/// comentário lá).
#[derive(Clone)]
pub struct OutputDevice {
    pub id: AudioObjectID,
    pub stream_id: AudioObjectID,
    pub name: String,
    /// Estável entre reinícios do `coreaudiod`, ao contrário do id.
    pub uid: String,
    pub caps: DeviceCaps,
    pub physical_formats: Vec<AudioStreamBasicDescription>,
    pub nominal_rate: f64,
    pub volume: f32, // negativo quando o device não expõe volume
}

fn same_rate(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.5
}

/// O device saiu do sistema: insistir numa escrita ou numa espera só gasta o teto inteiro.
fn device_gone(status: OSStatus) -> bool {
    status == kAudioHardwareBadObjectError as OSStatus
        || status == kAudioHardwareBadDeviceError as OSStatus
}

/// Só se restaura rate, formato e volume de um device vivo e ainda nosso. Escrever num device
/// que outro processo tomou mexeria no áudio dele; num device que sumiu, cada escrita só
/// espera o teto.
fn may_restore(hogged: bool, alive: bool, owner: Option<pid_t>, me: pid_t) -> bool {
    hogged && alive && owner == Some(me)
}

/// O erro de quando outro app já tem o device em modo exclusivo.
fn hog_busy_message(owner: pid_t, app: Option<&str>) -> String {
    format!(
        "o device está em uso exclusivo por {} (pid {owner})",
        app.unwrap_or("outro app")
    )
}

/// Quanto uma escrita de volume feita logo depois de reconfigurar o formato precisa esperar
/// antes de ser conferida. Ver `write_volume_confirmed`.
const VOLUME_SETTLE: Duration = Duration::from_millis(30);

/// O que falta dos `VOLUME_SETTLE` desde a escrita. Zero se o trabalho feito nesse meio-tempo
/// já cobriu a espera.
fn settle_remaining(written_at: Instant, now: Instant) -> Duration {
    VOLUME_SETTLE.saturating_sub(now.saturating_duration_since(written_at))
}

/// O resultado de uma conferência dentro de `wait_until`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Check {
    Ready,
    NotYet,
    /// O device sumiu: esperar mais só gastaria o prazo inteiro.
    Gone,
}

/// Confere `check` até ele ficar pronto ou `limit` passar. Entre uma conferência e outra
/// espera `interval` ou, com listener, a próxima notificação de `what`, o que vier primeiro.
fn wait_until(
    mut check: impl FnMut() -> Check,
    wake: Option<(&WatchState, Watched)>,
    interval: Duration,
    limit: Duration,
) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        // A geração é lida antes da conferência: uma notificação que chegue entre as duas
        // acorda a espera seguinte na hora, em vez de se perder.
        let seen = wake.map(|(watch, what)| watch.generation(what));
        match check() {
            Check::Ready => return true,
            Check::Gone => return false,
            Check::NotYet => {}
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        let pause = interval.min(remaining);
        match (wake, seen) {
            (Some((watch, what)), Some(seen)) => {
                watch.wait_for_change(what, seen, pause);
            }
            _ => std::thread::sleep(pause),
        }
    }
}

/// O HAL aceita a troca de rate de forma assíncrona: o set retorna sucesso antes de o
/// hardware assumir. Configurar o formato físico antes disso escreveria sobre um estado que
/// ainda vai mudar, então esperamos o device confirmar. Com listener, a notificação do rate
/// acorda a espera; o intervalo de 10 ms fica como reserva.
fn wait_for_rate(device: AudioObjectID, target: f64, watch: Option<&WatchState>) -> bool {
    let address = property_address(kAudioDevicePropertyNominalSampleRate, SCOPE_GLOBAL);
    wait_until(
        || match get_property::<f64>(device, &address) {
            Ok(current) if same_rate(current, target) => Check::Ready,
            Err(status) if device_gone(status) => Check::Gone,
            _ => Check::NotYet,
        },
        watch.map(|watch| (watch, Watched::NominalRate)),
        Duration::from_millis(10),
        Duration::from_secs(2),
    )
}

/// Uma escrita de volume da aquisição à espera da conferência.
pub struct PendingVolume {
    target: f32,
    written_at: Instant,
}

impl PendingVolume {
    pub fn target(&self) -> f32 {
        self.target
    }
}

/// Descreve o device de saída padrão.
pub fn query_default_output_device() -> Result<OutputDevice, String> {
    let id: AudioObjectID = get_property(
        kAudioObjectSystemObject,
        &property_address(kAudioHardwarePropertyDefaultOutputDevice, SCOPE_GLOBAL),
    )
    .map_err(|s| {
        format!(
            "não encontrei o device de saída padrão: {}",
            os_status_text(s)
        )
    })?;

    if id == kAudioObjectUnknown {
        return Err("não encontrei o device de saída padrão".to_string());
    }

    let name = get_string_property(
        id,
        &property_address(kAudioObjectPropertyName, SCOPE_GLOBAL),
    )
    .unwrap_or_default();
    let uid = get_string_property(
        id,
        &property_address(kAudioDevicePropertyDeviceUID, SCOPE_GLOBAL),
    )
    .unwrap_or_default();

    let streams: Vec<AudioStreamID> = get_property_array(
        id,
        &property_address(kAudioDevicePropertyStreams, SCOPE_OUTPUT),
    )
    .map_err(|s| format!("o device não expõe stream de saída: {}", os_status_text(s)))?;
    let Some(&stream_id) = streams.first() else {
        return Err("o device não expõe stream de saída".to_string());
    };

    let ranges: Vec<AudioValueRange> = get_property_array(
        id,
        &property_address(
            kAudioDevicePropertyAvailableNominalSampleRates,
            SCOPE_GLOBAL,
        ),
    )
    .map_err(|s| format!("não consegui listar os sample rates: {}", os_status_text(s)))?;

    let formats: Vec<AudioStreamRangedDescription> = get_property_array(
        stream_id,
        &property_address(kAudioStreamPropertyAvailablePhysicalFormats, SCOPE_GLOBAL),
    )
    .map_err(|s| {
        format!(
            "não consegui listar os formatos físicos: {}",
            os_status_text(s)
        )
    })?;

    let mut caps = DeviceCaps {
        rates: ranges
            .iter()
            .map(|r| SampleRateRange {
                minimum: r.mMinimum,
                maximum: r.mMaximum,
            })
            .collect(),
        physical_formats: Vec::new(),
        output_channels: 0,
    };
    let mut physical_formats = Vec::new();

    for ranged in &formats {
        let f = ranged.mFormat;
        if f.mFormatID != kAudioFormatLinearPCM {
            continue; // ignora passthrough tipo AC-3
        }
        caps.physical_formats.push(PhysicalFormatDesc {
            index: physical_formats.len() as i32,
            sample_rate: if f.mSampleRate > 0.0 {
                f.mSampleRate
            } else {
                ranged.mSampleRateRange.mMinimum
            },
            sample_type: if f.mFormatFlags & kAudioFormatFlagIsFloat != 0 {
                SampleType::Float
            } else {
                SampleType::Integer
            },
            bits_per_channel: f.mBitsPerChannel,
            channels: f.mChannelsPerFrame,
            // Formatos de faixa contínua trazem mSampleRate zerado e a faixa aqui. Colapsá-la
            // no mínimo faria o device recusar toda taxa válida que não fosse o extremo.
            sample_rate_minimum: ranged.mSampleRateRange.mMinimum,
            sample_rate_maximum: ranged.mSampleRateRange.mMaximum,
        });
        physical_formats.push(f);
    }

    if let Ok(current) = get_property::<AudioStreamBasicDescription>(
        stream_id,
        &property_address(kAudioStreamPropertyVirtualFormat, SCOPE_GLOBAL),
    ) {
        caps.output_channels = current.mChannelsPerFrame;
    }

    let nominal_rate = get_property::<f64>(
        id,
        &property_address(kAudioDevicePropertyNominalSampleRate, SCOPE_GLOBAL),
    )
    .unwrap_or(0.0);
    let volume = get_property::<f32>(
        id,
        &property_address(kAudioDevicePropertyVolumeScalar, SCOPE_OUTPUT),
    )
    .unwrap_or(-1.0);

    Ok(OutputDevice {
        id,
        stream_id,
        name,
        uid,
        caps,
        physical_formats,
        nominal_rate,
        volume,
    })
}

/// Quem tem o hog do device agora: −1 é ninguém; `None`, leitura que falhou.
pub fn hog_owner(device: AudioObjectID) -> Option<pid_t> {
    get_property::<pid_t>(
        device,
        &property_address(kAudioDevicePropertyHogMode, SCOPE_GLOBAL),
    )
    .ok()
}

/// O nome curto do processo, pela libproc. `None` se o processo já não existe.
pub fn process_name(pid: pid_t) -> Option<String> {
    unsafe extern "C" {
        // libproc, parte da libSystem: devolve o tamanho escrito, 0 se o processo não existe.
        fn proc_name(pid: i32, buffer: *mut std::ffi::c_void, buffersize: u32) -> i32;
    }
    let mut buffer = [0u8; 256];
    // SAFETY: o buffer é nosso e o tamanho passado é o dele.
    let written = unsafe { proc_name(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    let written = usize::try_from(written).ok().filter(|&n| n > 0)?;
    let name = buffer.get(..written)?;
    Some(String::from_utf8_lossy(name).into_owned())
}

/// Volume e mudo do device agora. `None` onde ele não tem o controle ou a leitura falhou.
pub fn live_volume(device: AudioObjectID) -> (Option<f32>, Option<bool>) {
    let scalar = get_property::<f32>(
        device,
        &property_address(kAudioDevicePropertyVolumeScalar, SCOPE_OUTPUT),
    )
    .ok();
    let muted = get_property::<u32>(
        device,
        &property_address(kAudioDevicePropertyMute, SCOPE_OUTPUT),
    )
    .ok()
    .map(|muted| muted != 0);
    (scalar, muted)
}

/// O que o HAL diz agora sobre o device e o stream: vivo, dono do hog, rate e formato físico.
pub fn live_signals(device: AudioObjectID, stream: AudioObjectID) -> LiveSignals {
    let alive = get_property::<u32>(
        device,
        &property_address(kAudioDevicePropertyDeviceIsAlive, SCOPE_GLOBAL),
    )
    .ok()
    .map(|alive| alive != 0);
    let nominal_rate = get_property::<f64>(
        device,
        &property_address(kAudioDevicePropertyNominalSampleRate, SCOPE_GLOBAL),
    )
    .ok();
    let physical = get_property::<AudioStreamBasicDescription>(
        stream,
        &property_address(kAudioStreamPropertyPhysicalFormat, SCOPE_GLOBAL),
    )
    .ok()
    .map(|format| PhysicalFormat::from_asbd(&format));
    LiveSignals {
        alive,
        hog_owner: hog_owner(device),
        nominal_rate,
        physical,
    }
}

/// Toma o device para uso exclusivo e trava rate e formato físico. O `Drop` devolve tudo ao
/// estado anterior — inclusive quando a aquisição falha no meio do caminho.
pub struct HoggedDevice {
    device_id: AudioObjectID,
    stream_id: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    running: bool,
    hogged: bool,
    /// `AudioDeviceStop` e `AudioDeviceDestroyIOProcID` que falharam. Com os dois falhando, o
    /// callback pode continuar vivo e quem segura o contexto dele não pode liberá-lo.
    stop_failed: bool,
    destroy_failed: bool,

    stream_format: AudioStreamBasicDescription,
    virtual_format_locked: bool,

    restore_error: Option<String>,
    original_volume: f32, // negativo enquanto nada foi alterado
    original_rate: f64,
    original_physical: AudioStreamBasicDescription,
    original_virtual: AudioStreamBasicDescription,
    saved_formats: bool,
    /// O device como ficou no fim da aquisição, lido do HAL: é contra isto que a perda é
    /// julgada.
    baseline: Option<Baseline>,
    /// Os listeners do device, para as esperas acordarem na notificação.
    watch: Option<Arc<WatchState>>,
}

impl Default for HoggedDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl HoggedDevice {
    pub fn new() -> Self {
        Self {
            device_id: kAudioObjectUnknown,
            stream_id: kAudioObjectUnknown,
            proc_id: None,
            running: false,
            hogged: false,
            stop_failed: false,
            destroy_failed: false,
            stream_format: AudioStreamBasicDescription::default(),
            virtual_format_locked: false,
            restore_error: None,
            original_volume: -1.0,
            original_rate: 0.0,
            original_physical: AudioStreamBasicDescription::default(),
            original_virtual: AudioStreamBasicDescription::default(),
            saved_formats: false,
            baseline: None,
            watch: None,
        }
    }

    pub fn with_watch(watch: Option<Arc<WatchState>>) -> Self {
        let mut hogged = Self::new();
        hogged.watch = watch;
        hogged
    }

    /// Um device "retido" que nunca passou por `acquire`: soltá-lo não chama o HAL. Serve aos
    /// testes do engine que simulam a perda sem hardware.
    #[cfg(test)]
    pub(crate) fn with_baseline_for_test(baseline: Baseline) -> Self {
        let mut hogged = Self::new();
        hogged.baseline = Some(baseline);
        hogged
    }

    pub fn device_id(&self) -> AudioObjectID {
        self.device_id
    }

    pub fn stream_id(&self) -> AudioObjectID {
        self.stream_id
    }

    pub fn baseline(&self) -> Option<Baseline> {
        self.baseline
    }

    /// Depois de `finish`: o IOProc pode continuar sendo chamado, e o contexto que ele lê
    /// tem de ficar vivo.
    pub fn io_proc_may_still_run(&self) -> bool {
        self.stop_failed && self.destroy_failed
    }

    pub fn acquire(
        &mut self,
        device: &OutputDevice,
        rate: f64,
        physical: &AudioStreamBasicDescription,
    ) -> Result<(), String> {
        // Uma segunda aquisição gravaria o estado já modificado por cima do original, e a
        // restauração passaria a devolver o device ao estado errado.
        if self.saved_formats || self.hogged {
            return Err("este device já foi tomado por esta instância".to_string());
        }

        self.device_id = device.id;
        self.stream_id = device.stream_id;

        // Antes de qualquer escrita: com o hog de outro app, escrever mexeria no áudio dele, e
        // nada aqui pode ficar marcado para restaurar. O nosso próprio pid também é erro —
        // reescrever o hog com o mesmo pid o desliga.
        let hog_address = property_address(kAudioDevicePropertyHogMode, SCOPE_GLOBAL);
        let me = std::process::id() as pid_t;
        match hog_owner(self.device_id) {
            Some(owner) if owner == me => {
                return Err("este processo já tem o device em modo exclusivo".to_string());
            }
            Some(owner) if owner > 0 => {
                return Err(hog_busy_message(owner, process_name(owner).as_deref()));
            }
            _ => {}
        }

        // `device.nominal_rate` pode ter sido lido com o hardware ainda retido por uma sessão
        // anterior: `Engine::load` consulta o device antes de soltá-lo, de propósito, para
        // poder negociar sem apagar a faixa em reprodução se a negociação falhar. Um valor
        // cacheado dessa consulta não é o rate original do hardware — é o rate que a própria
        // sessão anterior impôs. Confiar nele aqui gravaria o restaurar errado e, se coincidir
        // com o rate da faixa nova, também pularia a troca de rate (`same_rate` abaixo) com o
        // hardware já devolvido a outra taxa por baixo. A fonte da verdade tem que ser o
        // hardware agora, com o device já livre — `stop_and_release` sempre roda antes de
        // qualquer `acquire` —, tanto para o que restaurar quanto para decidir a troca.
        let rate_address = property_address(kAudioDevicePropertyNominalSampleRate, SCOPE_GLOBAL);
        let live_rate = get_property::<f64>(self.device_id, &rate_address).map_err(|_| {
            "não consegui ler o sample rate atual do device para poder restaurá-lo depois"
                .to_string()
        })?;
        self.original_rate = live_rate;

        let physical_address = property_address(kAudioStreamPropertyPhysicalFormat, SCOPE_GLOBAL);
        let virtual_address = property_address(kAudioStreamPropertyVirtualFormat, SCOPE_GLOBAL);

        let (Ok(original_physical), Ok(original_virtual)) = (
            get_property::<AudioStreamBasicDescription>(self.stream_id, &physical_address),
            get_property::<AudioStreamBasicDescription>(self.stream_id, &virtual_address),
        ) else {
            return Err(
                "não consegui ler o formato atual do device para poder restaurá-lo depois"
                    .to_string(),
            );
        };
        self.original_physical = original_physical;
        self.original_virtual = original_virtual;

        // O hog vem antes de mexer no formato: só o dono exclusivo troca o formato físico de
        // maneira confiável, e é ele que impede o mixer de somar outros clientes ao stream.
        let status = set_property(self.device_id, &hog_address, &me);
        if status != 0 {
            return Err(format!(
                "não consegui tomar o device em modo exclusivo: {}",
                os_status_text(status)
            ));
        }

        // O set pode devolver sucesso sem que o device seja nosso. Nesse caso nada aqui é nosso
        // para restaurar, nem o hog: soltá-lo poderia soltar o de outro app.
        let owner = hog_owner(self.device_id).unwrap_or(-1);
        if owner != me {
            return Err(if owner > 0 {
                hog_busy_message(owner, process_name(owner).as_deref())
            } else {
                "o device não confirmou o modo exclusivo".to_string()
            });
        }
        self.hogged = true;
        self.saved_formats = true;

        if !same_rate(live_rate, rate) {
            let status = set_property(self.device_id, &rate_address, &rate);
            if status != 0 {
                return Err(format!(
                    "o device recusou o sample rate: {}",
                    os_status_text(status)
                ));
            }
            if !wait_for_rate(self.device_id, rate, self.watch.as_deref()) {
                return Err(
                    "o device não assumiu o sample rate pedido dentro do tempo esperado"
                        .to_string(),
                );
            }
        }

        // Formatos de faixa contínua chegam com a taxa zerada; é aqui que ela vira concreta.
        let mut target = *physical;
        target.mSampleRate = rate;

        let status = set_property(self.stream_id, &physical_address, &target);
        if status != 0 {
            return Err(format!(
                "o device recusou o formato físico: {}",
                os_status_text(status)
            ));
        }
        // Como o rate, o formato físico pode ser aplicado depois de o set voltar. O virtual, o
        // formato de entrega e a linha de base só são lidos depois de ele assentar; lidos antes,
        // a base guardaria o formato velho e a notificação seguinte pararia a faixa como se
        // alguém tivesse mexido no device. Sem falhar no teto: um device que ajustou o pedido
        // nunca assenta nele, e aí vale o que ele relata.
        let requested = PhysicalFormat::from_asbd(&target);
        wait_until(
            || match get_property::<AudioStreamBasicDescription>(self.stream_id, &physical_address)
            {
                Ok(current) if PhysicalFormat::from_asbd(&current).same_as(&requested) => {
                    Check::Ready
                }
                Err(status) if device_gone(status) => Check::Gone,
                _ => Check::NotYet,
            },
            self.watch
                .as_deref()
                .map(|watch| (watch, Watched::PhysicalFormat)),
            Duration::from_millis(10),
            Duration::from_millis(250),
        );

        // O ideal é o IOProc ver exatamente o formato físico. Muitos devices recusam e mantêm
        // float32 no formato virtual — o que ainda preserva os bits, porque a conversão de
        // inteiro de até 24 bits para float32 e de volta é exata.
        self.virtual_format_locked = set_property(self.stream_id, &virtual_address, &target) == 0;

        self.stream_format = get_property(self.stream_id, &virtual_address).map_err(|_| {
            "não consegui ler o formato que o device vai entregar ao callback".to_string()
        })?;

        // A linha de base é o que o device assumiu, não o que pedimos: é contra ela que uma
        // mudança por fora é julgada depois.
        let (Ok(nominal_rate), Ok(physical)) = (
            get_property::<f64>(self.device_id, &rate_address),
            get_property::<AudioStreamBasicDescription>(self.stream_id, &physical_address),
        ) else {
            return Err(
                "não consegui ler o estado final do device depois de configurá-lo".to_string(),
            );
        };
        self.baseline = Some(Baseline {
            pid: me,
            nominal_rate,
            physical: PhysicalFormat::from_asbd(&physical),
        });
        Ok(())
    }

    pub fn stream_format(&self) -> AudioStreamBasicDescription {
        self.stream_format
    }

    pub fn virtual_format_locked(&self) -> bool {
        self.virtual_format_locked
    }

    /// Converte pela curva do próprio amplificador. Só existe neste sentido: a conversão
    /// inversa publicada pelo HAL não corresponde ao que o device de fato aplica.
    pub fn decibels_to_scalar(&self, decibels: f64) -> Option<f32> {
        let address = property_address(kAudioDevicePropertyVolumeDecibelsToScalar, SCOPE_OUTPUT);
        let mut value = decibels as f32;
        let mut size = std::mem::size_of::<f32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                self.device_id,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut value as *mut _ as *mut _,
            )
        };
        (status == 0).then_some(value)
    }

    /// Estado real do volume depois de aplicado: escalar e decibéis lidos do hardware.
    pub fn read_volume(&self) -> Option<(f32, f64)> {
        let scalar = get_property::<f32>(
            self.device_id,
            &property_address(kAudioDevicePropertyVolumeScalar, SCOPE_OUTPUT),
        )
        .ok()?;
        let decibels = get_property::<f32>(
            self.device_id,
            &property_address(kAudioDevicePropertyVolumeDecibels, SCOPE_OUTPUT),
        )
        .ok()?;
        Some((scalar, decibels as f64))
    }

    /// Escreve o volume e confirma lendo de volta. O HAL descarta a primeira escrita logo
    /// após uma reconfiguração de formato — devolvendo noErr e, pior, um valor de cache que
    /// faz a conferência imediata passar. Sem isto, o volume pedido pode simplesmente não
    /// valer, e o som sai no volume anterior.
    ///
    /// Por isso a espera de 30 ms antes de cada conferência: é o que torna a leitura capaz de
    /// discordar. Custa caro, então só o caminho de aquisição usa esta versão — uma vez por
    /// device tomado. Para as escritas do slider, ver `write_volume_now`.
    fn write_volume_confirmed(&self, target: f32) -> bool {
        let address = property_address(kAudioDevicePropertyVolumeScalar, SCOPE_OUTPUT);
        for _ in 0..20 {
            if device_gone(set_property(self.device_id, &address, &target)) {
                return false;
            }
            // A confirmação precisa vir depois de uma pausa: ler imediatamente devolve o
            // valor que acabamos de escrever, mesmo quando o device não o assumiu.
            std::thread::sleep(std::time::Duration::from_millis(30));
            match get_property::<f32>(self.device_id, &address) {
                Ok(current) if (current - target).abs() < 0.005 => return true,
                Err(status) if device_gone(status) => return false,
                _ => {}
            }
        }
        false
    }

    /// Escreve o volume com o device já estável e confere na hora, sem espera.
    ///
    /// O cache que obriga `write_volume_confirmed` a dormir aparece logo após reconfigurar o
    /// formato. Com o device estável o comportamento medido é outro: em 20 escritas isoladas,
    /// lidas só 250 ms depois — tarde demais para qualquer cache curto mascarar um descarte —,
    /// nenhuma foi perdida; e uma rajada de 30 escritas seguidas deixou o hardware no último
    /// valor pedido. A leitura de conferência custa 0,17 ms; a espera que ela substitui,
    /// 30 ms. Como o slider escreve a cada quadro do arrasto, essa diferença é a diferença
    /// entre o som acompanhar o controle e chegar segundos depois.
    fn write_volume_now(&self, target: f32) -> bool {
        let address = property_address(kAudioDevicePropertyVolumeScalar, SCOPE_OUTPUT);
        set_property(self.device_id, &address, &target);
        match get_property::<f32>(self.device_id, &address) {
            Ok(current) => (current - target).abs() < 0.005,
            Err(_) => false,
        }
    }

    /// Checagens comuns aos dois caminhos de escrita, incluindo guardar o volume anterior.
    fn prepare_volume_write(&mut self, scalar: f32) -> Result<f32, String> {
        let scalar = scalar.clamp(0.0, 1.0);
        let address = property_address(kAudioDevicePropertyVolumeScalar, SCOPE_OUTPUT);

        if !has_property(self.device_id, &address) {
            return Err("este device não expõe controle de volume".to_string());
        }
        if !is_property_settable(self.device_id, &address) {
            return Err("o volume deste device não é ajustável por software".to_string());
        }

        if self.original_volume < 0.0 {
            // Só a primeira mudança define o que restaurar.
            let current = get_property::<f32>(self.device_id, &address).map_err(|_| {
                "não consegui ler o volume atual para poder restaurá-lo depois".to_string()
            })?;
            self.original_volume = current;
        }
        Ok(scalar)
    }

    /// Ajusta o volume e guarda o anterior para devolvê-lo junto com o resto do estado.
    /// Chamar antes de `start`: depois, som já teria saído no volume antigo.
    ///
    /// Esta é a escrita da aquisição, logo depois de reconfigurar o formato — a única
    /// condição em que o HAL descarta a escrita em silêncio, então é aqui que a conferência
    /// cara se paga.
    pub fn set_volume(&mut self, scalar: f32) -> Result<(), String> {
        let pending = self.begin_volume(scalar)?;
        if !self.confirm_volume(&pending) {
            return Err(
                "o device não assumiu o volume pedido; não vou tocar sem essa garantia".to_string(),
            );
        }
        Ok(())
    }

    /// A primeira metade de `set_volume`: escreve e marca quando. Escrever aqui, logo depois
    /// do formato, mantém a ordem hog → rate → formato → volume; a conferência fica para
    /// `confirm_volume`, depois de o resto da partida ter corrido.
    pub fn begin_volume(&mut self, scalar: f32) -> Result<PendingVolume, String> {
        let scalar = self.prepare_volume_write(scalar)?;
        let address = property_address(kAudioDevicePropertyVolumeScalar, SCOPE_OUTPUT);
        if device_gone(set_property(self.device_id, &address, &scalar)) {
            return Err("o device de saída sumiu durante a partida".to_string());
        }
        Ok(PendingVolume {
            target: scalar,
            written_at: Instant::now(),
        })
    }

    /// Espera o que faltar dos 30 ms desde a escrita e confere. Se o device descartou a
    /// escrita, volta ao laço de `write_volume_confirmed`.
    pub fn confirm_volume(&self, pending: &PendingVolume) -> bool {
        std::thread::sleep(settle_remaining(pending.written_at, Instant::now()));
        let address = property_address(kAudioDevicePropertyVolumeScalar, SCOPE_OUTPUT);
        match get_property::<f32>(self.device_id, &address) {
            Ok(current) if (current - pending.target).abs() < 0.005 => true,
            Err(status) if device_gone(status) => false,
            _ => self.write_volume_confirmed(pending.target),
        }
    }

    /// Ajusta o volume durante a reprodução, com o device já configurado e estável — o
    /// caminho do slider. Confere na hora em vez de esperar; ver `write_volume_now`.
    pub fn set_volume_interactive(&mut self, scalar: f32) -> Result<(), String> {
        let scalar = self.prepare_volume_write(scalar)?;
        if !self.write_volume_now(scalar) {
            return Err("o device não assumiu o volume pedido".to_string());
        }
        Ok(())
    }

    /// Registra o callback e abre o fluxo.
    ///
    /// # Safety
    ///
    /// `context` é entregue ao Core Audio, que o repassa cru a cada disparo de `io_proc`.
    /// Quem chama garante que ele aponta para um valor vivo enquanto o callback puder
    /// disparar — isto é, ao menos até `stop()` retornar. O contrato não cabia no tipo antes
    /// de esta função ser `unsafe`, e ficava só em prosa no chamador.
    pub unsafe fn start(
        &mut self,
        io_proc: AudioDeviceIOProc,
        context: *mut std::ffi::c_void,
    ) -> Result<(), String> {
        let mut proc_id: AudioDeviceIOProcID = None;
        let status =
            unsafe { AudioDeviceCreateIOProcID(self.device_id, io_proc, context, &mut proc_id) };
        if status != 0 || proc_id.is_none() {
            return Err(format!(
                "não consegui registrar o callback de áudio: {}",
                os_status_text(status)
            ));
        }
        self.proc_id = proc_id;

        let status = unsafe { AudioDeviceStart(self.device_id, self.proc_id) };
        if status != 0 {
            return Err(format!(
                "não consegui iniciar a reprodução: {}",
                os_status_text(status)
            ));
        }
        self.running = true;
        Ok(())
    }

    pub fn stop(&mut self) {
        if !self.running {
            return;
        }
        let status = unsafe { AudioDeviceStop(self.device_id, self.proc_id) };
        self.stop_failed = status != 0;
        self.running = false;
    }

    /// Retoma sem recriar o IOProc. Chamar `start` de novo registraria um callback adicional
    /// e o anterior continuaria vivo, com os dois consumindo o mesmo ring buffer.
    pub fn resume(&mut self) -> Result<(), String> {
        if self.running {
            return Ok(());
        }
        if self.proc_id.is_none() {
            return Err("não há callback registrado para retomar".to_string());
        }
        let status = unsafe { AudioDeviceStart(self.device_id, self.proc_id) };
        if status != 0 {
            return Err(format!(
                "não consegui retomar a reprodução: {}",
                os_status_text(status)
            ));
        }
        self.running = true;
        Ok(())
    }

    /// Devolve o device ao estado original agora, em vez de esperar o `Drop`, e informa o que
    /// falhou. Chamar de novo (ou pelo `Drop`) não repete o trabalho.
    pub fn finish(&mut self) -> Result<(), String> {
        self.restore();
        match self.restore_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn restore(&mut self) {
        if self.device_id == kAudioObjectUnknown {
            return;
        }

        self.stop();
        if self.proc_id.is_some() {
            let status = unsafe { AudioDeviceDestroyIOProcID(self.device_id, self.proc_id) };
            self.destroy_failed = status != 0;
            self.proc_id = None;
        }

        let me = std::process::id() as pid_t;
        if !may_restore(
            self.hogged,
            crate::device_watch::is_alive(self.device_id),
            hog_owner(self.device_id),
            me,
        ) {
            // Sem erro: não há o que o usuário possa fazer, e a perda já foi relatada.
            self.saved_formats = false;
            self.original_volume = -1.0;
            self.hogged = false;
            return;
        }

        if self.saved_formats {
            // Espelha a ordem de acquire(): o rate primeiro, confirmado, e só então os
            // formatos. Aplicar formato enquanto o device ainda está no rate negociado
            // escreveria sobre um estado prestes a mudar.
            let rate_address =
                property_address(kAudioDevicePropertyNominalSampleRate, SCOPE_GLOBAL);
            if set_property(self.device_id, &rate_address, &self.original_rate) != 0
                || !wait_for_rate(self.device_id, self.original_rate, self.watch.as_deref())
            {
                self.restore_error =
                    Some("não consegui devolver o sample rate original ao device".to_string());
            }

            let physical_address =
                property_address(kAudioStreamPropertyPhysicalFormat, SCOPE_GLOBAL);
            if set_property(self.stream_id, &physical_address, &self.original_physical) != 0 {
                self.restore_error =
                    Some("não consegui devolver o formato físico original ao device".to_string());
            }
            let virtual_address = property_address(kAudioStreamPropertyVirtualFormat, SCOPE_GLOBAL);
            set_property(self.stream_id, &virtual_address, &self.original_virtual);
            self.saved_formats = false;
        }

        // Depois dos formatos, porque a reconfiguração deles descarta escritas de volume; e
        // ainda sob posse exclusiva, para que a escrita não dispute o device com outro
        // processo.
        if self.original_volume >= 0.0 {
            if !self.write_volume_confirmed(self.original_volume) {
                self.restore_error =
                    Some("não consegui devolver o volume original ao device".to_string());
            }
            self.original_volume = -1.0;
        }

        if self.hogged {
            let release: pid_t = -1;
            let hog_address = property_address(kAudioDevicePropertyHogMode, SCOPE_GLOBAL);
            if set_property(self.device_id, &hog_address, &release) != 0 {
                self.restore_error =
                    Some("não consegui liberar o modo exclusivo do device".to_string());
            }
            self.hogged = false;
        }
    }
}

impl Drop for HoggedDevice {
    fn drop(&mut self) {
        self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Requerem hardware e tomam o device por 1-2 segundos: rodar com
    //   cargo test -- --ignored --test-threads=1
    // Em paralelo eles disputam o mesmo device e reprovam sem que haja defeito nenhum — daí
    // o `--test-threads=1`, que o alvo `make hw-test` já traz.

    /// Lê o rate direto do device que o teste tem na mão.
    ///
    /// Nunca via `query_default_output_device()`: sob hog mode o macOS aponta o "default
    /// output" para OUTRO hardware, e a leitura volta o rate do device errado. Foi assim que
    /// `toma_o_device_e_restaura` passou a reprovar sem que houvesse defeito — ele conferia o
    /// rate das caixas embutidas enquanto segurava o fone.
    fn rate_do_hardware(id: AudioObjectID) -> f64 {
        get_property::<f64>(
            id,
            &property_address(kAudioDevicePropertyNominalSampleRate, SCOPE_GLOBAL),
        )
        .expect("ler o nominal rate do device")
    }

    /// Mesmo motivo de `rate_do_hardware`, para o volume.
    fn volume_do_hardware(id: AudioObjectID) -> f32 {
        get_property::<f32>(
            id,
            &property_address(kAudioDevicePropertyVolumeScalar, SCOPE_OUTPUT),
        )
        .expect("ler o volume do device")
    }
    #[test]
    fn a_espera_do_volume_desconta_o_trabalho_feito_depois_da_escrita() {
        let escrita = Instant::now();
        assert_eq!(settle_remaining(escrita, escrita), VOLUME_SETTLE);
        assert_eq!(
            settle_remaining(escrita, escrita + Duration::from_millis(12)),
            Duration::from_millis(18)
        );
        assert_eq!(
            settle_remaining(escrita, escrita + Duration::from_millis(45)),
            Duration::ZERO
        );
    }

    #[test]
    fn a_espera_acorda_na_notificacao_sem_esperar_o_intervalo() {
        let watch = Arc::new(WatchState::new());
        let pronto = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let notificador = {
            let watch = Arc::clone(&watch);
            let pronto = Arc::clone(&pronto);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                pronto.store(true, std::sync::atomic::Ordering::SeqCst);
                watch.record(Watched::NominalRate);
                watch.wake_waiters();
            })
        };
        let inicio = Instant::now();

        let assumiu = wait_until(
            || {
                if pronto.load(std::sync::atomic::Ordering::SeqCst) {
                    Check::Ready
                } else {
                    Check::NotYet
                }
            },
            Some((&watch, Watched::NominalRate)),
            Duration::from_secs(1),
            Duration::from_secs(5),
        );

        notificador
            .join()
            .expect("o notificador não deveria entrar em pânico");
        assert!(assumiu);
        assert!(
            inicio.elapsed() < Duration::from_millis(500),
            "{:?}",
            inicio.elapsed()
        );
    }

    #[test]
    fn a_espera_sem_listener_confere_no_intervalo_ate_o_prazo() {
        let mut conferencias = 0;
        let inicio = Instant::now();

        let assumiu = wait_until(
            || {
                conferencias += 1;
                Check::NotYet
            },
            None,
            Duration::from_millis(10),
            Duration::from_millis(60),
        );

        assert!(!assumiu);
        assert!(inicio.elapsed() >= Duration::from_millis(60));
        // Sob carga o `sleep` passa do pedido: o que importa é conferir mais de uma vez e parar
        // no prazo.
        assert!(
            (2..=8).contains(&conferencias),
            "{conferencias} conferências"
        );
    }

    #[test]
    fn a_espera_desiste_na_hora_se_o_device_sumiu() {
        let inicio = Instant::now();
        let assumiu = wait_until(
            || Check::Gone,
            None,
            Duration::from_millis(10),
            Duration::from_secs(2),
        );
        assert!(!assumiu);
        assert!(inicio.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn so_objeto_ou_device_invalido_contam_como_device_que_sumiu() {
        assert!(device_gone(kAudioHardwareBadObjectError as OSStatus));
        assert!(device_gone(kAudioHardwareBadDeviceError as OSStatus));
        assert!(!device_gone(0));
        assert!(!device_gone(kAudioHardwareUnknownPropertyError as OSStatus));
    }

    #[test]
    fn so_restaura_device_vivo_e_ainda_nosso() {
        let eu: pid_t = 4242;
        assert!(may_restore(true, true, Some(eu), eu));
        assert!(!may_restore(true, false, Some(eu), eu), "device que sumiu");
        assert!(
            !may_restore(true, true, Some(99), eu),
            "hog de outro processo"
        );
        assert!(!may_restore(true, true, Some(-1), eu), "hog de ninguém");
        assert!(!may_restore(true, true, None, eu), "dono ilegível");
        assert!(
            !may_restore(false, true, Some(eu), eu),
            "nunca tomado por nós"
        );
    }

    #[test]
    fn hog_ocupado_diz_o_app_e_o_pid() {
        let com_nome = hog_busy_message(99, Some("Spotifast"));
        let sem_nome = hog_busy_message(99, None);
        assert!(
            com_nome.contains("Spotifast") && com_nome.contains("99"),
            "{com_nome}"
        );
        assert!(sem_nome.contains("99"), "{sem_nome}");
    }

    #[test]
    fn o_nome_do_processo_vem_da_libproc() {
        let proprio = process_name(std::process::id() as pid_t);
        assert!(
            proprio.as_deref().is_some_and(|nome| !nome.is_empty()),
            "{proprio:?}"
        );
        assert_eq!(process_name(i32::MAX), None);
    }

    #[test]
    fn objeto_desconhecido_nao_esta_vivo() {
        let sinais = live_signals(kAudioObjectUnknown, kAudioObjectUnknown);
        assert_ne!(sinais.alive, Some(true));
        assert_eq!(sinais.hog_owner, None);
    }

    #[test]
    #[ignore]
    fn toma_o_device_e_restaura() {
        let device = query_default_output_device().expect("device");
        let rate_antes = device.nominal_rate;
        let alvo = if same_rate(rate_antes, 44100.0) {
            48000.0
        } else {
            44100.0
        };
        let idx = device
            .caps
            .physical_formats
            .iter()
            .position(|f| same_rate(f.sample_rate, alvo))
            .expect("device deveria oferecer formato no rate alvo");

        {
            let mut hogged = HoggedDevice::new();
            hogged
                .acquire(&device, alvo, &device.physical_formats[idx])
                .expect("acquire deveria funcionar");

            let agora = rate_do_hardware(device.id);
            assert!(
                same_rate(agora, alvo),
                "device deveria estar travado em {alvo}, está em {agora}"
            );
        } // Drop restaura

        std::thread::sleep(std::time::Duration::from_millis(500));
        let depois = rate_do_hardware(device.id);
        assert!(
            same_rate(depois, rate_antes),
            "rate deveria voltar a {rate_antes}, está em {depois}"
        );
    }

    #[test]
    #[ignore]
    fn acquire_ignora_nominal_rate_cacheado_e_le_o_hardware_ao_vivo() {
        // Reproduz, no nível onde a correção mora, o bug mais grave da onda: `Engine::load()`
        // consulta o device *antes* de `stop_and_release` devolvê-lo (de propósito, para
        // poder negociar sem apagar a faixa em reprodução se a negociação falhar) — então,
        // com uma faixa ainda tocando, essa consulta pode trazer um `OutputDevice.nominal_rate`
        // que é, na verdade, o rate que a própria sessão em curso impôs, não o rate original.
        // Uma reprodução fiel via `Engine` inteiro não é confiável neste hardware: com fone
        // conectado, o macOS aponta o "device de saída padrão" para as caixas embutidas
        // enquanto o fone está retido (o mesmo efeito descrito no README, seção Volume) — a
        // consulta feita durante o hog acerta outro device físico, não o mesmo device com um
        // rate desatualizado. Este teste isola só o mecanismo que `acquire()` de fato controla:
        // simula o `OutputDevice` "stale" que `load()` produziria, sem depender de qual device
        // o macOS resolve chamar de "padrão" nesse meio-tempo.
        let device = query_default_output_device().expect("device");
        let rate_original = device.nominal_rate;

        let rate_a = if same_rate(rate_original, 44100.0) {
            48000.0
        } else {
            44100.0
        };
        let rate_b = if same_rate(rate_a, 96000.0) || same_rate(rate_original, 96000.0) {
            88200.0
        } else {
            96000.0
        };
        assert!(
            !same_rate(rate_a, rate_b),
            "rate_a e rate_b precisam diferir para o teste fazer sentido"
        );

        let idx_a = device
            .caps
            .physical_formats
            .iter()
            .position(|f| same_rate(f.sample_rate, rate_a))
            .expect("device deveria oferecer formato no rate_a");
        let idx_b = device
            .caps
            .physical_formats
            .iter()
            .position(|f| same_rate(f.sample_rate, rate_b))
            .expect("device deveria oferecer formato no rate_b");

        // Sessão A: toma o device em rate_a e solta — o hardware volta a rate_original.
        {
            let mut hog_a = HoggedDevice::new();
            hog_a
                .acquire(&device, rate_a, &device.physical_formats[idx_a])
                .expect("acquire A deveria funcionar");
        } // Drop restaura

        std::thread::sleep(std::time::Duration::from_millis(500));
        let depois_de_a = query_default_output_device().expect("device").nominal_rate;
        assert!(
            same_rate(depois_de_a, rate_original),
            "pré-condição do teste falhou: deveria estar em {rate_original} depois da sessão \
             A, está em {depois_de_a}"
        );

        // Simula o `OutputDevice` que `load()` produziria se consultasse o device ainda retido
        // pela sessão A: mesmo id/stream/caps, mas `nominal_rate` travado no valor que a sessão
        // A impôs — o hardware real já voltou a `rate_original`, só este campo ficou
        // desatualizado, exatamente como uma consulta feita com o device ainda hogado faria.
        let mut device_stale = query_default_output_device().expect("device");
        device_stale.nominal_rate = rate_a;

        // Sessão B: adquire com o `OutputDevice` stale, pedindo um rate diferente de ambos.
        let mut hog_b = HoggedDevice::new();
        hog_b
            .acquire(&device_stale, rate_b, &device.physical_formats[idx_b])
            .expect("acquire B deveria funcionar");

        // A prova direta: `restore()` depois de setar o rate errado também seta o formato
        // físico salvo em `original_physical` — que É lido ao vivo, sem passar pelo campo
        // problemático —, e em muitos devices isso arrasta o rate nominal junto, mascarando o
        // sintoma observável no hardware. Por isso a checagem que não pode ser mascarada é no
        // campo privado que a correção passou a preencher com uma leitura ao vivo, não com
        // `device_stale.nominal_rate` (o valor propositalmente desatualizado, rate_a).
        // Acessível porque `tests` é submódulo de `device`, do mesmo jeito que
        // `engine::tests` acessa o campo privado `EngineInner::source`.
        assert!(
            same_rate(hog_b.original_rate, rate_original),
            "original_rate deveria ser o rate lido ao vivo do hardware ({rate_original} Hz), \
             não o nominal_rate cacheado passado em `device` ({rate_a} Hz); ficou em {}",
            hog_b.original_rate
        );

        drop(hog_b); // Drop restaura
        std::thread::sleep(std::time::Duration::from_millis(500));
        let rate_final = query_default_output_device().expect("device").nominal_rate;
        assert!(
            same_rate(rate_final, rate_original),
            "o device deveria voltar a {rate_original} Hz (o original de antes de qualquer \
             hog), está em {rate_final} Hz"
        );
    }

    #[test]
    #[ignore]
    fn a_linha_de_base_continua_valendo_depois_de_o_device_assentar() {
        // Uma linha de base lida antes de o formato assentar viraria perda falsa: o player
        // pararia sozinho no meio da faixa.
        let device = query_default_output_device().expect("device");
        let alvo = if same_rate(device.nominal_rate, 44100.0) {
            48000.0
        } else {
            44100.0
        };
        let idx = device
            .caps
            .physical_formats
            .iter()
            .position(|f| same_rate(f.sample_rate, alvo))
            .expect("device deveria oferecer formato no rate alvo");

        let mut hogged = HoggedDevice::new();
        hogged
            .acquire(&device, alvo, &device.physical_formats[idx])
            .expect("acquire deveria funcionar");
        let base = hogged
            .baseline()
            .expect("o acquire deveria gravar a linha de base");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let depois = live_signals(device.id, device.stream_id);

        assert_eq!(
            crate::recovery::verdict(&depois, &base),
            None,
            "{depois:?} x {base:?}"
        );
    }

    #[test]
    #[ignore]
    fn ajusta_e_restaura_o_volume() {
        let device = query_default_output_device().expect("device");
        if device.volume < 0.0 {
            eprintln!("pulando: device sem controle de volume");
            return;
        }
        let original = device.volume;
        let idx = device
            .caps
            .physical_formats
            .iter()
            .position(|f| same_rate(f.sample_rate, device.nominal_rate))
            .expect("formato no rate atual");

        {
            let mut hogged = HoggedDevice::new();
            hogged
                .acquire(&device, device.nominal_rate, &device.physical_formats[idx])
                .expect("acquire");
            hogged.set_volume(0.2).expect("set_volume");

            let (scalar, _) = hogged.read_volume().expect("read_volume");
            assert!(
                (scalar - 0.2).abs() < 0.005,
                "volume deveria ser 0.2, é {scalar}"
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
        let depois = volume_do_hardware(device.id);
        assert!(
            (depois - original).abs() < 0.01,
            "volume deveria voltar a {original}, está em {depois}"
        );
    }
}
