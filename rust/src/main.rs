mod device;
mod ffi;
mod format;
mod ring;
mod source;
mod volume;

use std::cell::UnsafeCell;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::Arc;

use coreaudio_sys::*;

use device::{query_default_output_device, HoggedDevice, OutputDevice};
use format::{validate_interleaved_format, FileFormat};
use ring::{aligned_read_size, RingBuffer};
use source::AudioSource;
use volume::{apply_ceiling, parse_volume, VolumeRequest, VolumeUnit};

const DEFAULT_CEILING: f64 = 0.5;
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

/// Estado compartilhado entre a thread que decodifica e o IOProc de tempo real.
struct Playback {
    ring: RingBuffer,
    producer_done: AtomicBool,
    finished: AtomicBool,
    underruns: AtomicU64,
    bytes_per_frame: u32,
    bytes_per_sample: u32, // por canal, incluindo o padding do container
    channels: u32,
    non_interleaved: bool,
    scratch: UnsafeCell<Vec<u8>>, // pré-alocado: o IOProc não pode alocar
}

// `scratch` só é tocado pelo consumidor, que é uma thread só — o IOProc na reprodução, ou o
// laço de dump. O contrato é o mesmo do RingBuffer: um produtor, um consumidor.
unsafe impl Sync for Playback {}
unsafe impl Send for Playback {}

/// Roda em thread de tempo real: só cópia e atômicos. Nada de alocar, travar ou imprimir.
unsafe extern "C" fn io_proc(
    _device: AudioObjectID,
    _now: *const AudioTimeStamp,
    _input_data: *const AudioBufferList,
    _input_time: *const AudioTimeStamp,
    output_data: *mut AudioBufferList,
    _output_time: *const AudioTimeStamp,
    context: *mut std::ffi::c_void,
) -> OSStatus {
    let p = &*(context as *const Playback);
    if output_data.is_null() || (*output_data).mNumberBuffers == 0 {
        return 0;
    }

    let buffer_count = (*output_data).mNumberBuffers as usize;
    let buffers = (*output_data).mBuffers.as_mut_ptr();

    if !p.non_interleaved {
        let b = &mut *buffers;
        let need = b.mDataByteSize as usize;
        let dst = std::slice::from_raw_parts_mut(b.mData as *mut u8, need);
        let take = aligned_read_size(need, p.ring.available_to_read(), p.bytes_per_frame);
        let got = p.ring.read(&mut dst[..take]);
        if got < need {
            // Silêncio é o único preenchimento seguro: lixo de memória enviado ao DAC vira
            // ruído branco em volume total.
            std::ptr::write_bytes(dst.as_mut_ptr().add(got), 0, need - got);
            if p.producer_done.load(Ordering::Acquire) {
                p.finished.store(true, Ordering::Release);
            } else {
                p.underruns.fetch_add(1, Ordering::Relaxed);
            }
        }
        return 0;
    }

    let frames = ((*buffers).mDataByteSize / p.bytes_per_sample) as usize;
    let need = frames * p.bytes_per_frame as usize;
    let scratch = &mut *p.scratch.get();
    if need > scratch.len() {
        // O device pediu mais do que reservamos: cala, não arrisca.
        for i in 0..buffer_count {
            let b = &mut *buffers.add(i);
            std::ptr::write_bytes(b.mData as *mut u8, 0, b.mDataByteSize as usize);
        }
        p.underruns.fetch_add(1, Ordering::Relaxed);
        return 0;
    }

    let take = aligned_read_size(need, p.ring.available_to_read(), p.bytes_per_frame);
    let got = p.ring.read(&mut scratch[..take]);
    if got < need {
        std::ptr::write_bytes(scratch.as_mut_ptr().add(got), 0, need - got);
    }

    // Desintercala: o ring guarda LRLRLR..., o device quer um buffer por canal. Buffers além
    // dos canais que temos recebem silêncio.
    let sample = p.bytes_per_sample as usize;
    let frame = p.bytes_per_frame as usize;
    for ch in 0..buffer_count {
        let b = &mut *buffers.add(ch);
        let dst = b.mData as *mut u8;
        if ch as u32 >= p.channels {
            std::ptr::write_bytes(dst, 0, b.mDataByteSize as usize);
            continue;
        }
        let src = scratch.as_ptr().add(ch * sample);
        for f in 0..frames {
            std::ptr::copy_nonoverlapping(src.add(f * frame), dst.add(f * sample), sample);
        }
    }

    if got < need {
        if p.producer_done.load(Ordering::Acquire) {
            p.finished.store(true, Ordering::Release);
        } else {
            p.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }
    0
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

fn describe_source(source: &AudioSource) -> String {
    let f: FileFormat = source.format();
    format!(
        "{} — {} Hz / {} bits / {} canais",
        source.codec_name(),
        f.sample_rate as i64,
        f.bit_depth,
        f.channels
    )
}

/// Descreve o volume que o device realmente assumiu, lido do hardware. A conversão de escalar
/// para decibéis publicada pelo HAL não bate com a curva aplicada de fato, então exibir o
/// valor convertido daria um número plausível e errado.
fn describe_applied_volume(hogged: &HoggedDevice) -> String {
    match hogged.read_volume() {
        Some((scalar, decibels)) => format!("{:.0}% ({:.1} dB)", scalar * 100.0, decibels),
        None => "desconhecido".to_string(),
    }
}

fn print_device_report(device: &OutputDevice, source: &AudioSource) {
    println!("arquivo  : {}", describe_source(source));
    println!("device   : {}", device.name);
    println!("rate atual: {} Hz", device.nominal_rate as i64);

    let rates: Vec<String> = device
        .caps
        .rates
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
        device.physical_formats.len()
    );
    for f in &device.physical_formats {
        println!("           {}", describe_format(f));
    }
    if device.volume >= 0.0 {
        println!("volume   : {:.0}%", device.volume * 100.0);
    }
}

/// Um pedido explícito é uma garantia: se não der para cumprir, é melhor não tocar do que
/// tocar mais alto do que se pediu. Já o teto é uma rede de proteção — não havendo controle
/// de volume, avisa e segue, que é o comportamento de sempre.
fn apply_volume(
    hogged: &mut HoggedDevice,
    device: &OutputDevice,
    request: Option<&VolumeRequest>,
    ceiling: f64,
) -> Result<(), String> {
    if let Some(request) = request {
        let scalar = match request.unit {
            VolumeUnit::Percent => request.value as f32,
            VolumeUnit::Decibels => hogged
                .decibels_to_scalar(request.value)
                .ok_or("este device não converte decibéis; use porcentagem")?,
        };
        let before = device.volume;
        hogged.set_volume(scalar)?;

        print!("volume   : {}", describe_applied_volume(hogged));
        if before >= 0.0 {
            print!(" [era {:.0}%]", before * 100.0);
        }
        println!();
        return Ok(());
    }

    if device.volume < 0.0 {
        println!("volume   : device sem controle de volume; teto não aplicável");
        return Ok(());
    }

    // O teto decide sobre o volume lido agora, não sobre o que havia antes de tomar o
    // device: entre uma coisa e outra o usuário pode ter mexido no volume, e a reconfiguração
    // do device também pode alterá-lo. Uma proteção que age sobre leitura velha não protege.
    let current = hogged
        .read_volume()
        .map(|(scalar, _)| scalar)
        .unwrap_or(device.volume);

    let decision = apply_ceiling(current as f64, ceiling);
    if !decision.apply {
        println!(
            "volume   : {} (abaixo do teto de {:.0}%)",
            describe_applied_volume(hogged),
            ceiling * 100.0
        );
        return Ok(());
    }

    if let Err(error) = hogged.set_volume(decision.scalar as f32) {
        println!(
            "aviso    : volume em {:.0}% e não consegui baixá-lo ({error})",
            current * 100.0
        );
        return Ok(());
    }
    println!(
        "volume   : {} [baixado do teto: estava em {:.0}%]",
        describe_applied_volume(hogged),
        current * 100.0
    );
    Ok(())
}

fn usage() -> i32 {
    eprint!(
        "uso: hog-audio [--info] [--volume V] [--max-volume V] [--dump ARQUIVO] <arquivo>\n\n\
         \x20 Reproduz o arquivo tomando o DAC em modo exclusivo, travado no sample\n\
         \x20 rate e no bit depth do próprio arquivo. Ctrl+C interrompe.\n\n\
         \x20 --info          mostra o que seria negociado, sem tocar no device\n\
         \x20 --volume V      volume da reprodução: 35, 35% ou -18dB\n\
         \x20 --max-volume V  teto aplicado quando --volume é omitido (padrão 50%)\n\
         \x20 --dump ARQUIVO  grava em disco os bytes que iriam ao DAC, sem tocar\n\n\
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
}

fn parse_args() -> Result<Options, i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut options = Options {
        path: String::new(),
        info_only: false,
        volume: None,
        ceiling: DEFAULT_CEILING,
        dump: None,
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
    // Antes de qualquer coisa que altere o device. Entre tomar o device e instalar os
    // handlers existiria uma janela em que um Ctrl+C mataria o processo pela disposição
    // padrão, sem rodar destrutor nenhum — e o sample rate ficaria trocado.
    // SIGHUP cobre o terminal sendo fechado; SIGQUIT, o Ctrl+\.
    for sig in [SIGINT, SIGTERM, SIGHUP, SIGQUIT] {
        unsafe { signal(sig, on_interrupt as usize) };
    }

    let options = match parse_args() {
        Ok(options) => options,
        Err(code) => return code,
    };

    let mut source = match AudioSource::open(&options.path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("erro: {error}");
            return 1;
        }
    };

    let device = match query_default_output_device() {
        Ok(device) => device,
        Err(error) => {
            eprintln!("erro: {error}");
            return 1;
        }
    };

    print_device_report(&device, &source);

    let file_format = source.format();
    let decision = format::negotiate(&file_format, &device.caps);
    if !decision.play {
        eprintln!("\nnão dá para reproduzir sem perda: {}", decision.reason);
        return 1;
    }
    println!("negociado: {}", decision.reason);
    if decision.duplicate_mono_to_stereo {
        println!("aviso    : arquivo mono, será duplicado nos dois canais");
    }
    if options.info_only {
        return 0;
    }

    let seconds = source.total_frames() as f64 / file_format.sample_rate;

    let mut hogged = HoggedDevice::new();
    let physical = device.physical_formats[decision.physical_format_index as usize];
    if let Err(error) = hogged.acquire(&device, decision.sample_rate, &physical) {
        eprintln!("erro: {error}");
        return 1;
    }

    // O volume é resolvido aqui, com o device já nosso e antes de qualquer amostra sair: é o
    // único ponto em que dá para garantir que o fone não receba o volume anterior.
    if let Err(error) =
        apply_volume(&mut hogged, &device, options.volume.as_ref(), options.ceiling)
    {
        eprintln!("erro: {error}");
        return 1;
    }

    let stream = hogged.stream_format();
    println!("exclusivo: sim (hog mode)");
    println!("físico   : {}", describe_format(&physical));
    println!(
        "callback : {}{}",
        describe_format(&stream),
        if hogged.virtual_format_locked() {
            " [travado igual ao físico]"
        } else {
            ""
        }
    );

    if stream.mFormatID != kAudioFormatLinearPCM {
        eprintln!("erro: o device não está em PCM linear; não vou alimentá-lo");
        return 1;
    }

    // O decodificador entrega sempre intercalado, que é como o ring buffer guarda; o IOProc
    // desintercala se o device pedir assim.
    //
    // O tamanho do frame vem do próprio device, nunca de bitsPerChannel/8: um formato pode
    // carregar amostras de 24 bits em containers de 32, e recalcular assumindo empacotamento
    // faria o decodificador produzir um passo e o IOProc ler outro — ruído branco, não música.
    let non_interleaved = stream.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0;
    let mut client = stream;
    client.mFormatFlags &= !kAudioFormatFlagIsNonInterleaved;
    client.mFramesPerPacket = 1;
    client.mBytesPerFrame = if non_interleaved {
        stream.mBytesPerFrame * stream.mChannelsPerFrame
    } else {
        stream.mBytesPerFrame
    };
    client.mBytesPerPacket = client.mBytesPerFrame;

    let check = validate_interleaved_format(
        client.mBitsPerChannel,
        client.mBytesPerFrame,
        client.mChannelsPerFrame,
    );
    if !check.ok {
        eprintln!("erro: formato de entrega inconsistente: {}", check.reason);
        return 1;
    }

    if let Err(error) = source.set_client_format(&client) {
        eprintln!("erro: {error}");
        return 1;
    }

    // O decodificador pode ajustar o que aceitou. Se o que ele vai entregar divergir do que
    // o device espera, parar aqui é a diferença entre silêncio e ruído em volume total.
    let effective = match source.effective_client_format() {
        Ok(effective) => effective,
        Err(error) => {
            eprintln!("erro: {error}");
            return 1;
        }
    };
    if effective.mBitsPerChannel != client.mBitsPerChannel
        || effective.mBytesPerFrame != client.mBytesPerFrame
        || effective.mChannelsPerFrame != client.mChannelsPerFrame
        || (effective.mFormatFlags & kAudioFormatFlagIsFloat)
            != (client.mFormatFlags & kAudioFormatFlagIsFloat)
        || (effective.mSampleRate - client.mSampleRate).abs() > 0.5
    {
        eprintln!(
            "erro: o decodificador vai entregar {}, mas o device espera {}; \
             reproduzir assim geraria ruído",
            describe_format(&effective),
            describe_format(&client)
        );
        return 1;
    }

    let ring_bytes = client.mSampleRate as usize * client.mBytesPerFrame as usize * 2;
    let playback = Arc::new(Playback {
        ring: RingBuffer::new(ring_bytes),
        producer_done: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        underruns: AtomicU64::new(0),
        bytes_per_frame: client.mBytesPerFrame,
        bytes_per_sample: client.mBytesPerFrame / client.mChannelsPerFrame,
        channels: client.mChannelsPerFrame,
        non_interleaved,
        scratch: UnsafeCell::new(vec![0u8; ring_bytes]),
    });

    let producer = {
        let playback = Arc::clone(&playback);
        let bytes_per_frame = client.mBytesPerFrame as usize;
        std::thread::spawn(move || {
            let mut chunk = vec![0u8; 64 * 1024];
            let frames_per_chunk = (chunk.len() / bytes_per_frame) as u32;
            while !interrupted() {
                let got = source.read(&mut chunk, frames_per_chunk);
                if got == 0 {
                    break;
                }
                let bytes = got as usize * bytes_per_frame;
                let mut written = 0usize;
                while written < bytes && !interrupted() {
                    written += playback.ring.write(&chunk[written..bytes]);
                    if written < bytes {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
            }
            playback.producer_done.store(true, Ordering::Release);
        })
    };

    let exit_code = match options.dump.as_ref() {
        Some(path) => run_dump(&playback, &client, path),
        None => run_playback(&mut hogged, &playback, seconds),
    };

    INTERRUPTED.store(1, Ordering::Relaxed); // desbloqueia a produtora
    let _ = producer.join();

    let underruns = playback.underruns.load(Ordering::Relaxed);
    if underruns > 0 {
        println!("aviso    : {underruns} falhas de alimentação do buffer");
    }

    if let Err(error) = hogged.finish() {
        eprintln!(
            "aviso    : {error}\n\
             \x20          o device pode ter ficado com outra configuração; tocar\n\
             \x20          qualquer outro som ou abrir Configuração de Áudio e MIDI ajusta"
        );
        return 1;
    }

    println!("fim      : device restaurado");
    exit_code
}

/// Consome o ring exatamente como o IOProc faria, mas grava em disco. O pipeline é o mesmo —
/// decodificador, ring buffer, alinhamento de frame — porque um atalho que apenas
/// decodificasse pularia justamente as partes onde estiveram os bugs mais caros.
fn run_dump(playback: &Arc<Playback>, client: &AudioStreamBasicDescription, path: &str) -> i32 {
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
            continue;
        }
        if playback.producer_done.load(Ordering::Acquire) || interrupted() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    println!("dump     : {frames_written} frames em {path}");
    0
}

fn run_playback(hogged: &mut HoggedDevice, playback: &Arc<Playback>, seconds: f64) -> i32 {
    // Deixa o buffer encher antes de abrir o fluxo, para o começo da faixa não sair picotado.
    let half = playback.ring.capacity() / 2;
    for _ in 0..200 {
        if playback.ring.available_to_read() >= half
            || playback.producer_done.load(Ordering::Acquire)
            || interrupted()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let context = Arc::as_ptr(playback) as *mut std::ffi::c_void;
    if let Err(error) = hogged.start(Some(io_proc), context) {
        eprintln!("erro: {error}");
        return 1;
    }

    println!("tocando  : {seconds:.1} s — Ctrl+C interrompe");

    while !interrupted() && !playback.finished.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let by_user = interrupted();
    hogged.stop();
    if by_user {
        130 // 128 + SIGINT, como manda a convenção
    } else {
        0
    }
}
