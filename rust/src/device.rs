//! Camada HAL: consulta do device, modo exclusivo, lock de rate e formato, volume, IOProc e
//! restauração do estado original.

use coreaudio_sys::*;

use crate::ffi::{
    cf_string_into_owned, get_property, get_property_array, has_property, is_property_settable,
    os_status_text, property_address, set_property,
};
use crate::format::{DeviceCaps, PhysicalFormatDesc, SampleRateRange, SampleType};

const SCOPE_GLOBAL: u32 = kAudioObjectPropertyScopeGlobal;
const SCOPE_OUTPUT: u32 = kAudioObjectPropertyScopeOutput;

/// O device de saída e tudo que ele aceita. `physical_formats` é paralelo a
/// `caps.physical_formats`: o núcleo escolhe um índice, aqui está o descritor correspondente.
pub struct OutputDevice {
    pub id: AudioObjectID,
    pub stream_id: AudioObjectID,
    pub name: String,
    pub caps: DeviceCaps,
    pub physical_formats: Vec<AudioStreamBasicDescription>,
    pub nominal_rate: f64,
    pub volume: f32, // negativo quando o device não expõe volume
}

fn same_rate(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.5
}

/// O HAL aceita a troca de rate de forma assíncrona: o set retorna sucesso antes de o
/// hardware assumir. Configurar o formato físico antes disso escreveria sobre um estado que
/// ainda vai mudar, então esperamos o device confirmar.
fn wait_for_rate(device: AudioObjectID, target: f64) -> bool {
    let address = property_address(kAudioDevicePropertyNominalSampleRate, SCOPE_GLOBAL);
    for _ in 0..200 {
        if let Ok(current) = get_property::<f64>(device, &address) {
            if same_rate(current, target) {
                return true;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

/// Descreve o device de saída padrão.
pub fn query_default_output_device() -> Result<OutputDevice, String> {
    let id: AudioObjectID = get_property(
        kAudioObjectSystemObject,
        &property_address(kAudioHardwarePropertyDefaultOutputDevice, SCOPE_GLOBAL),
    )
    .map_err(|s| format!("não encontrei o device de saída padrão: {}", os_status_text(s)))?;

    if id == kAudioObjectUnknown {
        return Err("não encontrei o device de saída padrão".to_string());
    }

    let name = get_property::<CFStringRef>(id, &property_address(kAudioObjectPropertyName, SCOPE_GLOBAL))
        .map(cf_string_into_owned)
        .unwrap_or_default();

    let streams: Vec<AudioStreamID> =
        get_property_array(id, &property_address(kAudioDevicePropertyStreams, SCOPE_OUTPUT))
            .map_err(|s| format!("o device não expõe stream de saída: {}", os_status_text(s)))?;
    let Some(&stream_id) = streams.first() else {
        return Err("o device não expõe stream de saída".to_string());
    };

    let ranges: Vec<AudioValueRange> = get_property_array(
        id,
        &property_address(kAudioDevicePropertyAvailableNominalSampleRates, SCOPE_GLOBAL),
    )
    .map_err(|s| format!("não consegui listar os sample rates: {}", os_status_text(s)))?;

    let formats: Vec<AudioStreamRangedDescription> = get_property_array(
        stream_id,
        &property_address(kAudioStreamPropertyAvailablePhysicalFormats, SCOPE_GLOBAL),
    )
    .map_err(|s| format!("não consegui listar os formatos físicos: {}", os_status_text(s)))?;

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

    let nominal_rate =
        get_property::<f64>(id, &property_address(kAudioDevicePropertyNominalSampleRate, SCOPE_GLOBAL))
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
        caps,
        physical_formats,
        nominal_rate,
        volume,
    })
}

/// Toma o device para uso exclusivo e trava rate e formato físico. O `Drop` devolve tudo ao
/// estado anterior — inclusive quando a aquisição falha no meio do caminho.
pub struct HoggedDevice {
    device_id: AudioObjectID,
    stream_id: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    running: bool,
    hogged: bool,

    stream_format: AudioStreamBasicDescription,
    virtual_format_locked: bool,

    restore_error: Option<String>,
    original_volume: f32, // negativo enquanto nada foi alterado
    original_rate: f64,
    original_physical: AudioStreamBasicDescription,
    original_virtual: AudioStreamBasicDescription,
    saved_formats: bool,
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
            stream_format: AudioStreamBasicDescription::default(),
            virtual_format_locked: false,
            restore_error: None,
            original_volume: -1.0,
            original_rate: 0.0,
            original_physical: AudioStreamBasicDescription::default(),
            original_virtual: AudioStreamBasicDescription::default(),
            saved_formats: false,
        }
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
        self.original_rate = device.nominal_rate;

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
        self.saved_formats = true;

        // O hog vem antes de mexer no formato: só o dono exclusivo troca o formato físico de
        // maneira confiável, e é ele que impede o mixer de somar outros clientes ao stream.
        let hog_address = property_address(kAudioDevicePropertyHogMode, SCOPE_GLOBAL);
        let me = std::process::id() as pid_t;
        let status = set_property(self.device_id, &hog_address, &me);
        if status != 0 {
            return Err(format!(
                "não consegui tomar o device em modo exclusivo: {}",
                os_status_text(status)
            ));
        }
        // Marcado antes de conferir o dono: se o set funcionou, a liberação é nossa obrigação
        // mesmo que a releitura discorde.
        self.hogged = true;

        let owner = get_property::<pid_t>(self.device_id, &hog_address).unwrap_or(-1);
        if owner != me {
            return Err(format!("o device já está tomado pelo processo {owner}"));
        }

        let rate_address = property_address(kAudioDevicePropertyNominalSampleRate, SCOPE_GLOBAL);
        if !same_rate(device.nominal_rate, rate) {
            let status = set_property(self.device_id, &rate_address, &rate);
            if status != 0 {
                return Err(format!(
                    "o device recusou o sample rate: {}",
                    os_status_text(status)
                ));
            }
            if !wait_for_rate(self.device_id, rate) {
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

        // O ideal é o IOProc ver exatamente o formato físico. Muitos devices recusam e mantêm
        // float32 no formato virtual — o que ainda preserva os bits, porque a conversão de
        // inteiro de até 24 bits para float32 e de volta é exata.
        self.virtual_format_locked = set_property(self.stream_id, &virtual_address, &target) == 0;

        self.stream_format = get_property(self.stream_id, &virtual_address).map_err(|_| {
            "não consegui ler o formato que o device vai entregar ao callback".to_string()
        })?;
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
    fn write_volume_confirmed(&self, target: f32) -> bool {
        let address = property_address(kAudioDevicePropertyVolumeScalar, SCOPE_OUTPUT);
        for _ in 0..20 {
            set_property(self.device_id, &address, &target);
            // A confirmação precisa vir depois de uma pausa: ler imediatamente devolve o
            // valor que acabamos de escrever, mesmo quando o device não o assumiu.
            std::thread::sleep(std::time::Duration::from_millis(30));
            if let Ok(current) = get_property::<f32>(self.device_id, &address) {
                if (current - target).abs() < 0.005 {
                    return true;
                }
            }
        }
        false
    }

    /// Ajusta o volume e guarda o anterior para devolvê-lo junto com o resto do estado.
    /// Chamar antes de `start`: depois, som já teria saído no volume antigo.
    pub fn set_volume(&mut self, scalar: f32) -> Result<(), String> {
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

        if !self.write_volume_confirmed(scalar) {
            return Err("o device não assumiu o volume pedido; não vou tocar sem essa garantia"
                .to_string());
        }
        Ok(())
    }

    pub fn start(
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
        unsafe { AudioDeviceStop(self.device_id, self.proc_id) };
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
            unsafe { AudioDeviceDestroyIOProcID(self.device_id, self.proc_id) };
            self.proc_id = None;
        }

        if self.saved_formats {
            // Espelha a ordem de acquire(): o rate primeiro, confirmado, e só então os
            // formatos. Aplicar formato enquanto o device ainda está no rate negociado
            // escreveria sobre um estado prestes a mudar.
            let rate_address =
                property_address(kAudioDevicePropertyNominalSampleRate, SCOPE_GLOBAL);
            if set_property(self.device_id, &rate_address, &self.original_rate) != 0
                || !wait_for_rate(self.device_id, self.original_rate)
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

            let agora = query_default_output_device().expect("device").nominal_rate;
            assert!(
                same_rate(agora, alvo),
                "device deveria estar travado em {alvo}, está em {agora}"
            );
        } // Drop restaura

        std::thread::sleep(std::time::Duration::from_millis(500));
        let depois = query_default_output_device().expect("device").nominal_rate;
        assert!(
            same_rate(depois, rate_antes),
            "rate deveria voltar a {rate_antes}, está em {depois}"
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
            assert!((scalar - 0.2).abs() < 0.005, "volume deveria ser 0.2, é {scalar}");
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
        let depois = query_default_output_device().expect("device").volume;
        assert!(
            (depois - original).abs() < 0.01,
            "volume deveria voltar a {original}, está em {depois}"
        );
    }
}
