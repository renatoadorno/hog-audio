use std::io::Write;
use std::sync::atomic::{AtomicI32, Ordering};

use coreaudio_sys::{AudioStreamBasicDescription, kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved};

use hog_audio::engine::{DeviceReport, Engine, LoadedTrack, VolumeOutcome, DEFAULT_CEILING};
use hog_audio::ring::aligned_read_size;
use hog_audio::transitions::PlayerState;
use hog_audio::volume::{parse_volume, VolumeRequest, VolumeUnit};

const DUMP_BLOCK_FRAMES: usize = 512;

// Declarado à mão em vez de trazer a crate libc: a comparação com o C++ exige dependência
// única, e o que precisamos daqui é uma função só.
unsafe extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
}

const SIGHUP: i32 = 1;
const SIGINT: i32 = 2;
const SIGQUIT: i32 = 3;
const SIGTERM: i32 = 15;

static INTERRUPTED: AtomicI32 = AtomicI32::new(0);

extern "C" fn on_interrupt(_sig: i32) {
    INTERRUPTED.store(1, Ordering::Relaxed); // a limpeza é do main
}

fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed) != 0
}

fn print_track_report(path: &str, track: &LoadedTrack) {
    println!("arquivo  : {path}");
    println!(
        "fonte    : {} {} Hz / {} bits / {} canais / {:.1} s",
        track.codec, track.sample_rate, track.bit_depth, track.channels, track.total_seconds
    );
    println!("device   : {}", track.device_name);
}

fn describe_format(f: &AudioStreamBasicDescription) -> String {
    let kind = if f.mFormatFlags & kAudioFormatFlagIsFloat != 0 {
        "float"
    } else {
        "inteiro"
    };
    let layout = if f.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0 {
        ", não intercalado"
    } else {
        ""
    };
    format!(
        "{} Hz / {} bits {} / {} canais{}",
        f.mSampleRate as i64, f.mBitsPerChannel, kind, f.mChannelsPerFrame, layout
    )
}

/// A ferramenta de diagnóstico do projeto: mostra o que o device oferece e o que foi
/// negociado, mesmo fora de `--info` — é como se responde "por que este arquivo não toca" e
/// "ele escolheu o formato físico certo". Fica fora de `print_track_report` de propósito:
/// não é parte do que a interface gráfica vai consumir, só da CLI.
fn print_device_report(report: &DeviceReport) {
    println!("rate atual: {} Hz", report.nominal_rate as i64);

    let rates: Vec<String> = report
        .supported_rates
        .iter()
        .map(|r| {
            if (r.maximum - r.minimum).abs() < 0.5 {
                format!("{}", r.minimum as i64)
            } else {
                format!("{}-{}", r.minimum as i64, r.maximum as i64)
            }
        })
        .collect();
    println!("rates    : {}", rates.join(", "));
    println!(
        "formatos : {} físicos disponíveis",
        report.physical_formats.len()
    );
    for f in &report.physical_formats {
        println!("           {}", describe_format(f));
    }
    if report.volume >= 0.0 {
        println!("volume   : {:.0}%", report.volume * 100.0);
    }
    println!("negociado: {}", report.negotiated);
    if report.duplicate_mono_to_stereo {
        println!("aviso    : arquivo mono, será duplicado nos dois canais");
    }
}

/// Formata o volume aplicado a partir do que foi de fato lido do hardware. `decibels` como
/// NaN é o sinal de que a leitura falhou — mesma situação que o antigo "desconhecido".
fn describe_volume_outcome(outcome: &VolumeOutcome) -> String {
    if outcome.decibels.is_nan() {
        "desconhecido".to_string()
    } else {
        format!("{:.0}% ({:.1} dB)", outcome.scalar * 100.0, outcome.decibels)
    }
}

/// Espelha as linhas que a CLI sempre imprimiu para o volume: pedido explícito, teto que não
/// precisou agir, teto que baixou o volume, teto que falhou ao baixar (não-fatal, mas o
/// motivo não pode se perder), ou device sem controle algum.
fn print_volume_outcome(outcome: &VolumeOutcome, explicit_request: bool, ceiling: f64) {
    if let Some(warning) = &outcome.warning {
        println!("aviso    : {warning}");
        return;
    }

    if !explicit_request && outcome.previous_scalar < 0.0 {
        println!("volume   : device sem controle de volume; teto não aplicável");
        return;
    }

    let applied = describe_volume_outcome(outcome);
    if explicit_request {
        print!("volume   : {applied}");
        if outcome.previous_scalar >= 0.0 {
            print!(" [era {:.0}%]", outcome.previous_scalar * 100.0);
        }
        println!();
    } else if outcome.lowered_by_ceiling {
        println!(
            "volume   : {applied} [baixado do teto: estava em {:.0}%]",
            outcome.previous_scalar * 100.0
        );
    } else {
        println!("volume   : {applied} (abaixo do teto de {:.0}%)", ceiling * 100.0);
    }
}

fn usage() -> i32 {
    eprint!(
        "uso: hog-audio [--info] [--volume V] [--max-volume V] [--dump ARQUIVO] <arquivo>\n\n\
         \x20 Reproduz o arquivo tomando o DAC em modo exclusivo, travado no sample\n\
         \x20 rate e no bit depth do próprio arquivo. Ctrl+C interrompe.\n\n\
         \x20 --info          mostra o que seria negociado, sem tocar no device\n\
         \x20 --volume V      volume da reprodução: 35, 35% ou -18dB\n\
         \x20 --max-volume V  teto aplicado quando --volume é omitido (padrão 50%)\n\
         \x20 --dump ARQUIVO  grava em disco os bytes que iriam ao DAC, sem tocar\n\
         \x20 --pause-at N    no modo dump, simula uma pausa depois de N frames\n\n\
         \x20 O volume é ajustado depois de tomar o device e antes de sair som, e é\n\
         \x20 devolvido ao valor anterior ao terminar.\n"
    );
    2
}

struct Options {
    path: String,
    info_only: bool,
    volume: Option<VolumeRequest>,
    ceiling: f64,
    dump: Option<String>,
    pause_at: Option<usize>,
}

fn parse_args() -> Result<Options, i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut options = Options {
        path: String::new(),
        info_only: false,
        volume: None,
        ceiling: DEFAULT_CEILING,
        dump: None,
        pause_at: None,
    };

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].clone();
        match arg.as_str() {
            "--info" => options.info_only = true,
            "-h" | "--help" => return Err(usage()),
            "--dump" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("erro: --dump exige um caminho de arquivo");
                    return Err(2);
                };
                options.dump = Some(value.clone());
            }
            "--pause-at" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("erro: --pause-at exige um número de frames");
                    return Err(2);
                };
                match value.parse::<usize>() {
                    Ok(frames) => options.pause_at = Some(frames),
                    Err(_) => {
                        eprintln!("erro: --pause-at espera um número inteiro de frames");
                        return Err(2);
                    }
                }
            }
            "--volume" | "--max-volume" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("erro: {arg} exige um valor");
                    return Err(2);
                };
                let parsed = parse_volume(value);
                if !parsed.valid {
                    eprintln!("erro: {}", parsed.reason);
                    return Err(2);
                }
                if arg == "--volume" {
                    options.volume = Some(parsed);
                } else if parsed.unit != VolumeUnit::Percent {
                    eprintln!("erro: --max-volume aceita só porcentagem");
                    return Err(2);
                } else {
                    options.ceiling = parsed.value;
                }
            }
            _ if options.path.is_empty() => options.path = arg,
            _ => return Err(usage()),
        }
        i += 1;
    }

    if options.path.is_empty() {
        return Err(usage());
    }
    Ok(options)
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    for sig in [SIGINT, SIGTERM, SIGHUP, SIGQUIT] {
        unsafe { signal(sig, on_interrupt as usize) };
    }

    let options = match parse_args() {
        Ok(options) => options,
        Err(code) => return code,
    };

    let engine = Engine::new();
    engine.set_requested_volume(options.volume.clone(), options.ceiling);

    let track = match engine.load(&options.path) {
        Ok(track) => track,
        Err(error) => {
            eprintln!("erro: {error}");
            return 1;
        }
    };

    print_track_report(&options.path, &track);
    if let Some(report) = engine.device_report() {
        print_device_report(&report);
    }
    if options.info_only {
        return 0;
    }

    if let Some(path) = options.dump.as_ref() {
        return run_dump(
            &engine,
            path,
            options.volume.is_some(),
            options.ceiling,
            options.pause_at,
        );
    }

    if let Err(error) = engine.play() {
        eprintln!("erro: {error}");
        return 1;
    }
    if let Some(outcome) = engine.volume_outcome() {
        print_volume_outcome(&outcome, options.volume.is_some(), options.ceiling);
    }
    println!("tocando  : {:.1} s — Ctrl+C interrompe", track.total_seconds);

    while !interrupted() && engine.state() == PlayerState::Playing {
        std::thread::sleep(std::time::Duration::from_millis(50));
        engine.poll_finished();
    }

    let by_user = interrupted();
    let underruns = engine.status().underruns();
    if underruns > 0 {
        println!("aviso    : {underruns} falhas de alimentação do buffer");
    }

    if let Err(error) = engine.shutdown() {
        eprintln!(
            "aviso    : {error}\n\
             \x20          o device pode ter ficado com outra configuração; tocar\n\
             \x20          qualquer outro som ou abrir Configuração de Áudio e MIDI ajusta"
        );
        return 1;
    }

    println!("fim      : device restaurado");
    if by_user {
        130
    } else {
        0
    }
}

/// Consome o ring exatamente como o IOProc faria, mas grava em disco. O pipeline é o mesmo —
/// decodificador, ring buffer, alinhamento de frame — porque um atalho que apenas
/// decodificasse pularia justamente as partes onde estiveram os bugs mais caros.
fn run_dump(
    engine: &Engine,
    path: &str,
    explicit_request: bool,
    ceiling: f64,
    pause_at: Option<usize>,
) -> i32 {
    let (playback, client) = match engine.start_offline() {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("erro: {error}");
            return 1;
        }
    };
    // O dump adquire o device e aplica o volume de verdade, igual à reprodução: omitir esta
    // linha esconderia algo que de fato aconteceu no hardware.
    if let Some(outcome) = engine.volume_outcome() {
        print_volume_outcome(&outcome, explicit_request, ceiling);
    }

    let mut file = match std::fs::File::create(path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("erro: não consegui criar {path}: {error}");
            return 1;
        }
    };

    let block_bytes = DUMP_BLOCK_FRAMES * client.mBytesPerFrame as usize;
    let mut block = vec![0u8; block_bytes];
    let mut frames_written: u64 = 0;
    let mut already_paused = false;

    loop {
        let take = aligned_read_size(
            block_bytes,
            playback.ring.available_to_read(),
            playback.bytes_per_frame,
        );
        let got = playback.ring.read(&mut block[..take]);
        if got > 0 {
            if let Err(error) = file.write_all(&block[..got]) {
                eprintln!("erro: falha ao gravar o dump: {error}");
                return 1;
            }
            frames_written += got as u64 / client.mBytesPerFrame as u64;
            // Reproduz o que o pause de verdade faz: o consumidor simplesmente deixa de ser
            // chamado por um tempo. O ring buffer não é tocado, a produtora enche e bloqueia, e
            // ao voltar a leitura continua no byte seguinte. Se algum dia a pausa passar a
            // descartar ou reiniciar o buffer, a saída deixa de bater e este teste reprova.
            if let Some(limit) = pause_at {
                if !already_paused && frames_written >= limit as u64 {
                    already_paused = true;
                    std::thread::sleep(std::time::Duration::from_millis(400));
                }
            }
            continue;
        }
        if playback.producer_done.load(Ordering::Acquire) || interrupted() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    println!("dump     : {frames_written} frames em {path}");

    if let Err(error) = engine.shutdown() {
        eprintln!("aviso    : {error}");
        return 1;
    }
    0
}
