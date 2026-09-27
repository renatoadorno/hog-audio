//! Listeners de propriedade do Core Audio: o device sumiu, alguém tomou o hog, o rate, o
//! formato, o volume ou o mudo mudaram por fora.
//!
//! Regras deste módulo:
//! - O callback nunca chama o HAL. Ele soma a geração da propriedade e acorda quem espera;
//!   quem consome lê o valor ao vivo. Ler dentro do callback reentraria no HAL pela thread de
//!   notificação dele e pegaria os valores intermediários do nosso próprio acquire/restore.
//! - Nenhuma chamada ao HAL com o mutex de um `WatchState` seguro: o callback precisa dele
//!   para acordar quem espera.
//! - Toda espera tem teto. Uma notificação que não chega não pode travar um comando.
//! - Os listeners nunca são removidos. O `AudioHardware.h` não garante que
//!   `AudioObjectRemovePropertyListener` espere um callback em andamento, e liberar o contexto
//!   depois de remover abriria um uso após liberação. O registro é permanente, um por device
//!   (id mais UID), e o contexto de cada um é vazado de propósito.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use coreaudio_sys::*;

use crate::ffi::{
    add_property_listener, get_property, get_string_property, has_property, os_status_text,
    property_address,
};

/// O que o player acompanha. Cada propriedade tem a própria geração: quem espera o rate não
/// acorda por causa do volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watched {
    Alive,
    HogMode,
    NominalRate,
    Streams,
    Volume,
    Mute,
    PhysicalFormat,
    Devices,
}

const WATCHED_COUNT: usize = 8;

impl Watched {
    fn index(self) -> usize {
        match self {
            Watched::Alive => 0,
            Watched::HogMode => 1,
            Watched::NominalRate => 2,
            Watched::Streams => 3,
            Watched::Volume => 4,
            Watched::Mute => 5,
            Watched::PhysicalFormat => 6,
            Watched::Devices => 7,
        }
    }

    fn from_selector(selector: u32) -> Option<Watched> {
        #[allow(non_upper_case_globals)]
        match selector {
            kAudioDevicePropertyDeviceIsAlive => Some(Watched::Alive),
            kAudioDevicePropertyHogMode => Some(Watched::HogMode),
            kAudioDevicePropertyNominalSampleRate => Some(Watched::NominalRate),
            kAudioDevicePropertyStreams => Some(Watched::Streams),
            kAudioDevicePropertyVolumeScalar => Some(Watched::Volume),
            kAudioDevicePropertyMute => Some(Watched::Mute),
            kAudioStreamPropertyPhysicalFormat => Some(Watched::PhysicalFormat),
            kAudioHardwarePropertyDevices => Some(Watched::Devices),
            _ => None,
        }
    }

    /// Onde o listener mora: no device, no stream ou no objeto do sistema.
    fn address(self) -> AudioObjectPropertyAddress {
        let (selector, scope) = match self {
            Watched::Alive => (
                kAudioDevicePropertyDeviceIsAlive,
                kAudioObjectPropertyScopeGlobal,
            ),
            Watched::HogMode => (kAudioDevicePropertyHogMode, kAudioObjectPropertyScopeGlobal),
            Watched::NominalRate => (
                kAudioDevicePropertyNominalSampleRate,
                kAudioObjectPropertyScopeGlobal,
            ),
            Watched::Streams => (kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput),
            Watched::Volume => (
                kAudioDevicePropertyVolumeScalar,
                kAudioObjectPropertyScopeOutput,
            ),
            Watched::Mute => (kAudioDevicePropertyMute, kAudioObjectPropertyScopeOutput),
            Watched::PhysicalFormat => (
                kAudioStreamPropertyPhysicalFormat,
                kAudioObjectPropertyScopeGlobal,
            ),
            Watched::Devices => (
                kAudioHardwarePropertyDevices,
                kAudioObjectPropertyScopeGlobal,
            ),
        };
        property_address(selector, scope)
    }
}

/// Registradas no device. Volume e mudo são opcionais: há device sem esses controles.
const DEVICE_PROPERTIES: [(Watched, bool); 6] = [
    (Watched::Alive, true),
    (Watched::HogMode, true),
    (Watched::NominalRate, true),
    (Watched::Streams, true),
    (Watched::Volume, false),
    (Watched::Mute, false),
];

/// As gerações de um device. Compartilhado entre o callback do HAL, o poll da interface e
/// quem espera uma mudança.
pub struct WatchState {
    generations: [AtomicU64; WATCHED_COUNT],
    total: AtomicU64,
    wake_lock: Mutex<()>,
    wake: Condvar,
}

impl Default for WatchState {
    fn default() -> Self {
        Self::new()
    }
}

impl WatchState {
    pub fn new() -> Self {
        Self {
            generations: std::array::from_fn(|_| AtomicU64::new(0)),
            total: AtomicU64::new(0),
            wake_lock: Mutex::new(()),
            wake: Condvar::new(),
        }
    }

    pub fn generation(&self, what: Watched) -> u64 {
        self.generations[what.index()].load(Ordering::Acquire)
    }

    /// Soma de todas as gerações: um valor só para saber se algo mudou.
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Acquire)
    }

    /// Soma das gerações que podem indicar a perda do device. Volume e mudo ficam de fora: o
    /// slider os muda a cada quadro do arrasto.
    pub fn device_generation(&self) -> u64 {
        [
            Watched::Alive,
            Watched::HogMode,
            Watched::NominalRate,
            Watched::Streams,
            Watched::PhysicalFormat,
            Watched::Devices,
        ]
        .into_iter()
        .map(|what| self.generation(what))
        .sum()
    }

    /// O que o callback faz por endereço recebido. Exposto ao crate para os testes do engine
    /// simularem uma notificação sem hardware.
    pub(crate) fn record(&self, what: Watched) {
        self.generations[what.index()].fetch_add(1, Ordering::AcqRel);
        self.total.fetch_add(1, Ordering::AcqRel);
    }

    /// Chamado depois de `record`. Tomar o mutex para avisar é o que impede a notificação de
    /// se perder entre a checagem de quem espera e o `wait` dele.
    pub(crate) fn wake_waiters(&self) {
        let _guard = self
            .wake_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.wake.notify_all();
    }

    /// Espera a geração de `what` sair de `seen`, até `limit`. `false` se o teto chegou antes.
    pub fn wait_for_change(&self, what: Watched, seen: u64, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        let mut guard = self
            .wake_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        loop {
            if self.generation(what) != seen {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            guard = self
                .wake
                .wait_timeout(guard, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// O callback registrado no HAL.
///
/// # Safety
///
/// `context` é nulo ou aponta para um `WatchState` vivo para sempre (ver `register_device`);
/// `addresses` é nulo ou aponta para `count` endereços contíguos, como o HAL entrega.
unsafe extern "C" fn on_property_change(
    _object: AudioObjectID,
    count: u32,
    addresses: *const AudioObjectPropertyAddress,
    context: *mut std::ffi::c_void,
) -> OSStatus {
    // Um pânico não pode atravessar a fronteira com o C do HAL.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if context.is_null() || addresses.is_null() {
            return;
        }
        // SAFETY: o contexto é o `Arc<WatchState>` vazado em `register_device`, que nunca é
        // devolvido; os endereços vêm do HAL, `count` deles, contíguos.
        let state = unsafe { &*(context as *const WatchState) };
        let addresses = unsafe { std::slice::from_raw_parts(addresses, count as usize) };
        let mut recorded = false;
        for what in addresses
            .iter()
            .filter_map(|address| Watched::from_selector(address.mSelector))
        {
            state.record(what);
            recorded = true;
        }
        if recorded {
            state.wake_waiters();
        }
    }));
    0
}

struct Registration {
    streams: Vec<AudioObjectID>,
    state: Arc<WatchState>,
}

/// O mesmo id pode voltar para outro hardware depois de um reinício do `coreaudiod`; o UID
/// não. Por isso a chave leva os dois.
type RegistryKey = (AudioObjectID, String);

fn registry() -> &'static Mutex<HashMap<RegistryKey, Registration>> {
    static REGISTRY: OnceLock<Mutex<HashMap<RegistryKey, Registration>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// O `WatchState` do device, registrando os listeners na primeira vez. As chamadas seguintes
/// para o mesmo device devolvem o mesmo estado; um stream novo ganha o listener de formato.
///
/// O mutex do registro fica seguro durante as chamadas ao HAL: o callback nunca o toma, então
/// não há como uma notificação esperar por ele.
pub fn watch(device: AudioObjectID, stream: AudioObjectID) -> Result<Arc<WatchState>, String> {
    let uid = get_string_property(
        device,
        &property_address(
            kAudioDevicePropertyDeviceUID,
            kAudioObjectPropertyScopeGlobal,
        ),
    )
    .map_err(|status| {
        format!(
            "não consegui ler o UID do device: {}",
            os_status_text(status)
        )
    })?;
    if !is_alive(device) {
        return Err("o device não está mais disponível".to_string());
    }

    let mut registry = registry().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(registration) = registry.get_mut(&(device, uid.clone())) {
        if !registration.streams.contains(&stream) {
            listen(
                stream,
                Watched::PhysicalFormat,
                Arc::as_ptr(&registration.state),
            )?;
            registration.streams.push(stream);
        }
        return Ok(Arc::clone(&registration.state));
    }

    let state = Arc::new(WatchState::new());
    register_device(device, stream, &state)?;
    registry.insert(
        (device, uid),
        Registration {
            streams: vec![stream],
            state: Arc::clone(&state),
        },
    );
    Ok(state)
}

pub fn is_alive(device: AudioObjectID) -> bool {
    get_property::<u32>(device, &Watched::Alive.address()).is_ok_and(|alive| alive != 0)
}

fn register_device(
    device: AudioObjectID,
    stream: AudioObjectID,
    state: &Arc<WatchState>,
) -> Result<(), String> {
    // Uma referência vazada: o HAL pode chamar o callback com este ponteiro a qualquer
    // momento, para sempre, porque o listener nunca é removido.
    let context = Arc::into_raw(Arc::clone(state));
    for (what, required) in DEVICE_PROPERTIES {
        if !required && !has_property(device, &what.address()) {
            continue;
        }
        listen(device, what, context)?;
    }
    listen(stream, Watched::PhysicalFormat, context)?;
    listen(kAudioObjectSystemObject, Watched::Devices, context)
}

fn listen(object: AudioObjectID, what: Watched, context: *const WatchState) -> Result<(), String> {
    // SAFETY: `context` aponta para um `WatchState` com uma referência vazada em
    // `register_device`, e o listener nunca é removido: o ponteiro vale para sempre.
    unsafe {
        add_property_listener(
            object,
            &what.address(),
            Some(on_property_change),
            context as *mut std::ffi::c_void,
        )
    }
    .map_err(|status| {
        format!(
            "não consegui acompanhar {what:?} do device: {}",
            os_status_text(status)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::property_address;

    fn endereco(selector: u32) -> AudioObjectPropertyAddress {
        property_address(selector, kAudioObjectPropertyScopeGlobal)
    }

    /// Chama o callback como o HAL chamaria, com os endereços dados.
    fn notificar(state: &Arc<WatchState>, selectors: &[u32]) {
        let enderecos: Vec<AudioObjectPropertyAddress> =
            selectors.iter().map(|&s| endereco(s)).collect();
        let status = unsafe {
            on_property_change(
                0,
                enderecos.len() as u32,
                enderecos.as_ptr(),
                Arc::as_ptr(state) as *mut std::ffi::c_void,
            )
        };
        assert_eq!(status, 0);
    }

    #[test]
    fn cada_propriedade_tem_a_propria_geracao() {
        let state = WatchState::new();
        state.record(Watched::Volume);
        state.record(Watched::Volume);

        assert_eq!(state.generation(Watched::Volume), 2);
        assert_eq!(state.generation(Watched::Alive), 0);
        assert_eq!(state.total(), 2);
    }

    #[test]
    fn o_callback_soma_a_geracao_de_cada_endereco_e_ignora_o_resto() {
        let state = Arc::new(WatchState::new());

        notificar(
            &state,
            &[
                kAudioDevicePropertyVolumeScalar,
                kAudioDevicePropertyMute,
                kAudioObjectPropertyName,
            ],
        );

        assert_eq!(state.generation(Watched::Volume), 1);
        assert_eq!(state.generation(Watched::Mute), 1);
        assert_eq!(state.total(), 2);
    }

    #[test]
    fn o_callback_reconhece_todas_as_propriedades_registradas() {
        let pares = [
            (kAudioDevicePropertyDeviceIsAlive, Watched::Alive),
            (kAudioDevicePropertyHogMode, Watched::HogMode),
            (kAudioDevicePropertyNominalSampleRate, Watched::NominalRate),
            (kAudioDevicePropertyStreams, Watched::Streams),
            (kAudioDevicePropertyVolumeScalar, Watched::Volume),
            (kAudioDevicePropertyMute, Watched::Mute),
            (kAudioStreamPropertyPhysicalFormat, Watched::PhysicalFormat),
            (kAudioHardwarePropertyDevices, Watched::Devices),
        ];
        for (selector, esperado) in pares {
            let state = Arc::new(WatchState::new());
            notificar(&state, &[selector]);
            assert_eq!(state.generation(esperado), 1, "{esperado:?}");
            assert_eq!(state.total(), 1, "{esperado:?}");
        }
    }

    #[test]
    fn o_callback_sem_contexto_ou_sem_enderecos_nao_faz_nada() {
        let endereco = endereco(kAudioDevicePropertyVolumeScalar);
        let status = unsafe { on_property_change(0, 1, &endereco, std::ptr::null_mut()) };
        assert_eq!(status, 0);

        let state = Arc::new(WatchState::new());
        let contexto = Arc::as_ptr(&state) as *mut std::ffi::c_void;
        let status = unsafe { on_property_change(0, 3, std::ptr::null(), contexto) };
        assert_eq!(status, 0);
        assert_eq!(state.total(), 0);
    }

    #[test]
    fn esperar_sem_mudanca_volta_no_teto() {
        let state = WatchState::new();
        let inicio = Instant::now();

        let mudou = state.wait_for_change(Watched::Volume, 0, Duration::from_millis(40));

        let decorrido = inicio.elapsed();
        assert!(!mudou);
        assert!(decorrido >= Duration::from_millis(40), "{decorrido:?}");
        assert!(decorrido < Duration::from_secs(1), "{decorrido:?}");
    }

    #[test]
    fn esperar_acorda_quando_a_propriedade_muda() {
        let state = Arc::new(WatchState::new());
        let vista = state.generation(Watched::NominalRate);
        let notificador = {
            let state = Arc::clone(&state);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                notificar(&state, &[kAudioDevicePropertyNominalSampleRate]);
            })
        };
        let inicio = Instant::now();

        let mudou = state.wait_for_change(Watched::NominalRate, vista, Duration::from_secs(5));

        notificador
            .join()
            .expect("o notificador não deveria entrar em pânico");
        assert!(mudou);
        assert!(
            inicio.elapsed() < Duration::from_secs(1),
            "{:?}",
            inicio.elapsed()
        );
    }

    #[test]
    fn mudanca_de_outra_propriedade_nao_acorda_quem_espera() {
        let state = Arc::new(WatchState::new());
        let notificador = {
            let state = Arc::clone(&state);
            std::thread::spawn(move || notificar(&state, &[kAudioDevicePropertyMute]))
        };

        let mudou = state.wait_for_change(Watched::Volume, 0, Duration::from_millis(80));

        notificador
            .join()
            .expect("o notificador não deveria entrar em pânico");
        assert!(!mudou);
        assert_eq!(state.generation(Watched::Mute), 1);
    }

    #[test]
    #[ignore = "precisa de um device de saída real; mexe no volume dele por um instante"]
    fn escrever_volume_no_device_padrao_sobe_a_geracao_de_volume() {
        let device = crate::device::query_default_output_device().expect("device");
        if device.volume < 0.0 {
            eprintln!("pulando: device sem controle de volume");
            return;
        }
        let state = watch(device.id, device.stream_id).expect("deveria registrar");
        let de_novo = watch(device.id, device.stream_id).expect("deveria reaproveitar");
        assert!(
            Arc::ptr_eq(&state, &de_novo),
            "o registro deveria ser único"
        );

        let endereco = property_address(
            kAudioDevicePropertyVolumeScalar,
            kAudioObjectPropertyScopeOutput,
        );
        let original = device.volume;
        let alvo = if original > 0.5 {
            original - 0.02
        } else {
            original + 0.02
        };
        let vista = state.generation(Watched::Volume);

        crate::ffi::set_property(device.id, &endereco, &alvo);
        let mudou = state.wait_for_change(Watched::Volume, vista, Duration::from_secs(2));
        crate::ffi::set_property(device.id, &endereco, &original);

        assert!(mudou, "a escrita de volume não gerou notificação");
    }

    /// O `AudioHardwareDeprecated.h` diz que, desde o 10.6, o HAL entrega as notificações pelo
    /// run loop principal, que a CLI e os testes nunca rodam; com `kAudioHardwarePropertyRunLoop`
    /// em NULL, ele usa uma thread própria. No macOS 27 a propriedade já vem NULL (verificado em
    /// 26/09/2026). Se um dia deixar de vir, os listeners param de chegar fora da interface.
    #[test]
    fn o_hal_entrega_as_notificacoes_em_thread_propria() {
        let run_loop = get_property::<CFRunLoopRef>(
            kAudioObjectSystemObject,
            &property_address(
                kAudioHardwarePropertyRunLoop,
                kAudioObjectPropertyScopeGlobal,
            ),
        )
        .expect("o HAL deveria responder sobre o run loop");
        assert!(run_loop.is_null(), "o HAL continua no run loop principal");
    }

    #[test]
    fn mudanca_antes_de_esperar_nao_se_perde() {
        let state = Arc::new(WatchState::new());
        let vista = state.generation(Watched::HogMode);
        notificar(&state, &[kAudioDevicePropertyHogMode]);

        assert!(state.wait_for_change(Watched::HogMode, vista, Duration::from_millis(10)));
    }
}
