//! Experimento do Passo 6b: uma escrita de volume que o HAL descarta gera notificação?
//!
//! A escrita logo depois de reconfigurar o formato é descartada às vezes, e a leitura imediata
//! devolve cache. Hoje a conferência espera 30 ms para poder discordar. Trocar essa espera
//! pela notificação só é seguro se nenhuma escrita descartada for notificada — senão a
//! notificação confirmaria um volume que o device não assumiu.
//!
//! Por ciclo, dois casos, cada um com uma escrita única e medida:
//! - aquisição: logo depois de tomar o device num rate diferente do original;
//! - restauração: logo depois de devolver rate e formato ao original, ainda em hog.
//!
//! Registra se veio notificação (e em quanto tempo), a leitura imediata e a leitura 250 ms
//! depois, que é a verdade: tarde demais para um cache curto mascarar o descarte.
//!
//! Toma o device de saída padrão por alguns minutos, sem tocar som. Volumes de 5 a 8%.
//!
//!   cargo test --release --test volume_notification -- --ignored --nocapture

use std::time::{Duration, Instant};

use coreaudio_sys::*;
use hog_audio::device::{HoggedDevice, query_default_output_device};
use hog_audio::device_watch::{Watched, watch};
use hog_audio::ffi::{get_property, property_address, set_property};

const CYCLES: usize = 200;
const TRUTH_AFTER: Duration = Duration::from_millis(250);

#[derive(Default)]
struct Tally {
    writes: usize,
    discarded: usize,
    discarded_notified: usize,
    effective_not_notified: usize,
    immediate_lied: usize,
    latencies_ms: Vec<f64>,
}

impl Tally {
    fn report(&self, name: &str) {
        let mut latencies = self.latencies_ms.clone();
        latencies.sort_by(f64::total_cmp);
        let median = latencies
            .get(latencies.len() / 2)
            .copied()
            .unwrap_or(f64::NAN);
        let max = latencies.last().copied().unwrap_or(f64::NAN);
        eprintln!(
            "{name}: {} escritas, {} descartadas, {} descartadas COM notificação, \
             {} efetivas SEM notificação, {} leituras imediatas que mentiram; \
             notificação em {median:.1} ms (mediana), {max:.1} ms (máximo)",
            self.writes,
            self.discarded,
            self.discarded_notified,
            self.effective_not_notified,
            self.immediate_lied
        );
    }
}

fn volume_address() -> AudioObjectPropertyAddress {
    property_address(
        kAudioDevicePropertyVolumeScalar,
        kAudioObjectPropertyScopeOutput,
    )
}

fn read_volume(device: AudioObjectID) -> f32 {
    get_property::<f32>(device, &volume_address()).expect("ler o volume")
}

/// Uma escrita única, medida: notificação, leitura imediata e leitura depois de `TRUTH_AFTER`.
fn measure_write(
    device: AudioObjectID,
    state: &hog_audio::device_watch::WatchState,
    target: f32,
    tally: &mut Tally,
) {
    let seen = state.generation(Watched::Volume);
    let written_at = Instant::now();
    set_property(device, &volume_address(), &target);
    let immediate = read_volume(device);
    let notified = state.wait_for_change(Watched::Volume, seen, TRUTH_AFTER);
    let notified_after = written_at.elapsed();
    std::thread::sleep(TRUTH_AFTER.saturating_sub(written_at.elapsed()));
    let truth = read_volume(device);

    let effective = (truth - target).abs() < 0.005;
    tally.writes += 1;
    if (immediate - target).abs() < 0.005 && !effective {
        tally.immediate_lied += 1;
    }
    match (effective, notified) {
        (false, true) => {
            tally.discarded += 1;
            tally.discarded_notified += 1;
        }
        (false, false) => tally.discarded += 1,
        (true, false) => tally.effective_not_notified += 1,
        (true, true) => tally
            .latencies_ms
            .push(notified_after.as_secs_f64() * 1000.0),
    }
}

/// Devolve o volume de antes do experimento ao sair, inclusive num pânico. As escritas medidas
/// são cruas de propósito: passar pelo `HoggedDevice` faria uma escrita a mais antes delas, e
/// a medida deixaria de ser a da primeira escrita depois do formato.
struct RestoreVolume {
    device: AudioObjectID,
    original: f32,
}

impl Drop for RestoreVolume {
    fn drop(&mut self) {
        for _ in 0..20 {
            set_property(self.device, &volume_address(), &self.original);
            std::thread::sleep(Duration::from_millis(50));
            if (read_volume(self.device) - self.original).abs() < 0.005 {
                return;
            }
        }
        eprintln!("aviso: não consegui devolver o volume a {}", self.original);
    }
}

fn wait_rate(device: AudioObjectID, target: f64) {
    let address = property_address(
        kAudioDevicePropertyNominalSampleRate,
        kAudioObjectPropertyScopeGlobal,
    );
    for _ in 0..200 {
        if get_property::<f64>(device, &address).is_ok_and(|rate| (rate - target).abs() < 0.5) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("o device não assumiu {target} Hz");
}

#[test]
#[ignore = "experimento: toma o device de saída por alguns minutos"]
fn escrita_de_volume_descartada_gera_notificacao() {
    let device = query_default_output_device().expect("device");
    assert!(
        device.volume >= 0.0,
        "o device precisa ter controle de volume"
    );
    let state = watch(device.id, device.stream_id).expect("listeners");
    let _restore_volume = RestoreVolume {
        device: device.id,
        original: device.volume,
    };
    let original_rate = device.nominal_rate;
    let physical_address = property_address(
        kAudioStreamPropertyPhysicalFormat,
        kAudioObjectPropertyScopeGlobal,
    );
    let original_physical: AudioStreamBasicDescription =
        get_property(device.stream_id, &physical_address).expect("formato original");
    let rates: Vec<f64> = [48_000.0, 88_200.0]
        .into_iter()
        .filter(|rate| {
            (rate - original_rate).abs() > 0.5
                && device
                    .caps
                    .physical_formats
                    .iter()
                    .any(|format| (format.sample_rate - rate).abs() < 0.5)
        })
        .collect();
    assert!(!rates.is_empty(), "sem rate alternativo para trocar");

    let mut aquisicao = Tally::default();
    let mut restauracao = Tally::default();
    for cycle in 0..CYCLES {
        let rate = rates[cycle % rates.len()];
        let index = device
            .caps
            .physical_formats
            .iter()
            .position(|format| (format.sample_rate - rate).abs() < 0.5)
            .expect("formato no rate");
        let mut hogged = HoggedDevice::with_watch(Some(state.clone()));
        hogged
            .acquire(&device, rate, &device.physical_formats[index])
            .expect("acquire");

        let target_a = if cycle % 2 == 0 { 0.05 } else { 0.06 };
        measure_write(device.id, &state, target_a, &mut aquisicao);

        let rate_address = property_address(
            kAudioDevicePropertyNominalSampleRate,
            kAudioObjectPropertyScopeGlobal,
        );
        set_property(device.id, &rate_address, &original_rate);
        wait_rate(device.id, original_rate);
        set_property(device.stream_id, &physical_address, &original_physical);
        let target_b = if cycle % 2 == 0 { 0.08 } else { 0.07 };
        measure_write(device.id, &state, target_b, &mut restauracao);

        drop(hogged);
        if cycle % 20 == 19 {
            eprintln!("{} ciclos", cycle + 1);
        }
    }

    aquisicao.report("aquisição");
    restauracao.report("restauração");
    assert_eq!(aquisicao.writes, CYCLES);
}
