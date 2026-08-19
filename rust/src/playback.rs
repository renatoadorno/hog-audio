//! Estado compartilhado entre a thread que decodifica e o IOProc de tempo real, mais o
//! próprio callback.
//!
//! Tudo que roda dentro do `io_proc` obedece à disciplina de tempo real: só cópia de memória
//! e operações atômicas. Alocar, travar um mutex ou imprimir aqui produz falhas audíveis e
//! intermitentes, que não aparecem em teste.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use coreaudio_sys::*;

use crate::ring::{aligned_read_size, RingBuffer};
use crate::status::SharedStatus;

/// Estado compartilhado entre a thread que decodifica e o IOProc de tempo real.
pub struct Playback {
    pub ring: RingBuffer,
    pub producer_done: AtomicBool,
    pub finished: AtomicBool,
    pub status: Arc<SharedStatus>,
    pub bytes_per_frame: u32,
    pub bytes_per_sample: u32, // por canal, incluindo o padding do container
    pub channels: u32,
    pub non_interleaved: bool,
    pub scratch: UnsafeCell<Vec<u8>>, // pré-alocado: o IOProc não pode alocar
}

// `scratch` só é tocado pelo consumidor, que é uma thread só — o IOProc na reprodução, ou o
// laço de dump. O contrato é o mesmo do RingBuffer: um produtor, um consumidor.
unsafe impl Sync for Playback {}
unsafe impl Send for Playback {}

/// Roda em thread de tempo real: só cópia e atômicos. Nada de alocar, travar ou imprimir.
pub unsafe extern "C" fn io_proc(
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
        p.status.add_frames((need / p.bytes_per_frame as usize) as u64);
        if got < need {
            // Silêncio é o único preenchimento seguro: lixo de memória enviado ao DAC vira
            // ruído branco em volume total.
            std::ptr::write_bytes(dst.as_mut_ptr().add(got), 0, need - got);
            if p.producer_done.load(Ordering::Acquire) {
                p.finished.store(true, Ordering::Release);
            } else {
                p.status.add_underrun();
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
        p.status.add_underrun();
        return 0;
    }

    let take = aligned_read_size(need, p.ring.available_to_read(), p.bytes_per_frame);
    let got = p.ring.read(&mut scratch[..take]);
    p.status.add_frames(frames as u64);
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
            p.status.add_underrun();
        }
    }
    0
}

impl Playback {
    pub fn new(
        capacity_bytes: usize,
        client: &AudioStreamBasicDescription,
        non_interleaved: bool,
        status: Arc<SharedStatus>,
    ) -> Self {
        Self {
            ring: RingBuffer::new(capacity_bytes),
            producer_done: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            status,
            bytes_per_frame: client.mBytesPerFrame,
            bytes_per_sample: client.mBytesPerFrame / client.mChannelsPerFrame,
            channels: client.mChannelsPerFrame,
            non_interleaved,
            scratch: UnsafeCell::new(vec![0u8; capacity_bytes]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `AudioBufferList` é de tamanho variável: a struct gerada declara só
    /// `mBuffers: [AudioBuffer; 1]`, mas o layout real do Core Audio tem `mNumberBuffers`
    /// elementos contíguos. `Vec<u64>` garante alinhamento de 8 bytes — o que a struct exige
    /// por causa do ponteiro em `AudioBuffer` — sem depender de quanto o alocador de `u8`
    /// decide alinhar na prática.
    struct SyntheticBufferList {
        storage: Vec<u64>,
    }

    impl SyntheticBufferList {
        fn new(buffers: &[AudioBuffer]) -> Self {
            let bytes = std::mem::size_of::<AudioBufferList>()
                + buffers.len().saturating_sub(1) * std::mem::size_of::<AudioBuffer>();
            let mut storage = vec![0u64; bytes.div_ceil(std::mem::size_of::<u64>())];
            let list = storage.as_mut_ptr() as *mut AudioBufferList;
            unsafe {
                (*list).mNumberBuffers = buffers.len() as UInt32;
                let first = std::ptr::addr_of_mut!((*list).mBuffers) as *mut AudioBuffer;
                for (i, b) in buffers.iter().enumerate() {
                    first.add(i).write(*b);
                }
            }
            Self { storage }
        }

        fn as_mut_ptr(&mut self) -> *mut AudioBufferList {
            self.storage.as_mut_ptr() as *mut AudioBufferList
        }
    }

    /// Um `AudioBuffer` apontando para memória que o teste possui e pode inspecionar depois
    /// do callback rodar.
    fn buffer(data: &mut [u8]) -> AudioBuffer {
        AudioBuffer {
            mNumberChannels: 1,
            mDataByteSize: data.len() as UInt32,
            mData: data.as_mut_ptr() as *mut std::ffi::c_void,
        }
    }

    /// Formato de cliente mínimo para os testes: `bytes_per_sample` sempre inteiro para não
    /// obscurecer a aritmética de offset que os testes de desintercalação verificam.
    fn client_format(bytes_per_frame: u32, channels: u32) -> AudioStreamBasicDescription {
        AudioStreamBasicDescription {
            mSampleRate: 100.0,
            mFormatID: 0,
            mFormatFlags: 0,
            mBytesPerPacket: bytes_per_frame,
            mFramesPerPacket: 1,
            mBytesPerFrame: bytes_per_frame,
            mChannelsPerFrame: channels,
            mBitsPerChannel: (bytes_per_frame / channels) * 8,
            mReserved: 0,
        }
    }

    /// Chama o callback fora do Core Audio, como a thread de tempo real chamaria — só que com
    /// um `AudioBufferList` sintético em vez de um device de verdade.
    fn call_io_proc(playback: &Playback, list: *mut AudioBufferList) {
        unsafe {
            io_proc(
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                list,
                std::ptr::null(),
                playback as *const Playback as *mut std::ffi::c_void,
            );
        }
    }

    #[test]
    fn ramo_intercalado_entrega_bytes_do_ring_na_ordem_certa() {
        let status = Arc::new(SharedStatus::new());
        let client = client_format(4, 2);
        let playback = Playback::new(64, &client, false, Arc::clone(&status));

        let pattern: Vec<u8> = (0u8..32).collect(); // 8 frames de 4 bytes, ring cheio o bastante
        assert_eq!(playback.ring.write(&pattern), pattern.len());

        let mut out = vec![0xAAu8; pattern.len()];
        let mut list = SyntheticBufferList::new(&[buffer(&mut out)]);
        call_io_proc(&playback, list.as_mut_ptr());

        assert_eq!(out, pattern);
    }

    #[test]
    fn ramo_intercalado_preenche_silencio_no_underrun_e_conta_a_falha() {
        let status = Arc::new(SharedStatus::new());
        status.set_track(1_000, 100.0);
        let client = client_format(4, 2);
        let playback = Playback::new(64, &client, false, Arc::clone(&status));

        let pattern: Vec<u8> = (0u8..8).collect(); // só 2 frames disponíveis
        assert_eq!(playback.ring.write(&pattern), pattern.len());

        // Sentinela não-zero: distingue "escreveu silêncio" de "não escreveu nada".
        let mut out = vec![0xFFu8; 32]; // device pede 8 frames
        let mut list = SyntheticBufferList::new(&[buffer(&mut out)]);
        call_io_proc(&playback, list.as_mut_ptr());

        assert_eq!(&out[..8], &pattern[..]);
        assert!(out[8..].iter().all(|&b| b == 0));
        assert_eq!(status.underruns(), 1);
        assert!(!playback.finished.load(Ordering::Acquire));
        // need (8 frames pedidos) != got (2 frames lidos) de propósito: só assim a contagem
        // consegue provar que segue o pedido, e não o lido, tem como reprovar sob mutação.
        assert!((status.elapsed_seconds() - 0.08).abs() < 1e-9);
    }

    #[test]
    fn ramo_intercalado_marca_fim_sem_contar_underrun_quando_produtora_terminou() {
        let status = Arc::new(SharedStatus::new());
        let client = client_format(4, 2);
        let playback = Playback::new(64, &client, false, Arc::clone(&status));
        playback.producer_done.store(true, Ordering::Release);
        // ring vazio de propósito: é o fim real da faixa, não uma falha de alimentação

        let mut out = vec![0xFFu8; 32];
        let mut list = SyntheticBufferList::new(&[buffer(&mut out)]);
        call_io_proc(&playback, list.as_mut_ptr());

        assert!(out.iter().all(|&b| b == 0));
        assert!(playback.finished.load(Ordering::Acquire));
        assert_eq!(status.underruns(), 0);
    }

    #[test]
    fn ramo_nao_intercalado_desintercala_lr_para_buffers_separados() {
        let status = Arc::new(SharedStatus::new());
        // 2 bytes por amostra, de propósito: com largura 1 o offset `ch * sample` é igual a
        // `ch`, e um bug que esquecesse de multiplicar pela largura passaria despercebido.
        let client = client_format(4, 2);
        let playback = Playback::new(64, &client, true, Arc::clone(&status));

        // LRLRLR..., 3 frames de 2 bytes por canal; os dois bytes de cada amostra são
        // diferentes entre si, para que inverter a ordem deles também reprove o teste.
        let interleaved = [
            0x10, 0x11, 0x20, 0x21, // frame 0: L, R
            0x12, 0x13, 0x22, 0x23, // frame 1: L, R
            0x14, 0x15, 0x24, 0x25, // frame 2: L, R
        ];
        assert_eq!(playback.ring.write(&interleaved), interleaved.len());

        let mut left = vec![0xFFu8; 6];
        let mut right = vec![0xFFu8; 6];
        let mut list = SyntheticBufferList::new(&[buffer(&mut left), buffer(&mut right)]);
        call_io_proc(&playback, list.as_mut_ptr());

        assert_eq!(left, vec![0x10, 0x11, 0x12, 0x13, 0x14, 0x15]);
        assert_eq!(right, vec![0x20, 0x21, 0x22, 0x23, 0x24, 0x25]);
    }

    #[test]
    fn ramo_nao_intercalado_zera_buffers_alem_dos_canais_existentes() {
        let status = Arc::new(SharedStatus::new());
        let client = client_format(4, 2); // 2 canais, 2 bytes por amostra
        let playback = Playback::new(64, &client, true, Arc::clone(&status));

        let interleaved = [
            0x10, 0x11, 0x20, 0x21, 0x12, 0x13, 0x22, 0x23, 0x14, 0x15, 0x24, 0x25,
        ];
        assert_eq!(playback.ring.write(&interleaved), interleaved.len());

        let mut left = vec![0xFFu8; 6];
        let mut right = vec![0xFFu8; 6];
        let mut extra = vec![0xEEu8; 6]; // device pediu 3 buffers, só há 2 canais
        let mut list =
            SyntheticBufferList::new(&[buffer(&mut left), buffer(&mut right), buffer(&mut extra)]);
        call_io_proc(&playback, list.as_mut_ptr());

        assert_eq!(left, vec![0x10, 0x11, 0x12, 0x13, 0x14, 0x15]);
        assert_eq!(right, vec![0x20, 0x21, 0x22, 0x23, 0x24, 0x25]);
        assert_eq!(extra, vec![0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn ramo_nao_intercalado_preenche_silencio_no_underrun_e_conta_a_falha() {
        let status = Arc::new(SharedStatus::new());
        status.set_track(1_000, 100.0);
        let client = client_format(4, 2);
        let playback = Playback::new(64, &client, true, Arc::clone(&status));

        // só 2 dos 4 frames que o device vai pedir estão disponíveis no ring
        let interleaved = [0x10, 0x11, 0x20, 0x21, 0x12, 0x13, 0x22, 0x23];
        assert_eq!(playback.ring.write(&interleaved), interleaved.len());

        // Sentinela não-zero: distingue "escreveu silêncio" de "não escreveu nada".
        let mut left = vec![0xFFu8; 8]; // device pede 4 frames
        let mut right = vec![0xFFu8; 8];
        let mut list = SyntheticBufferList::new(&[buffer(&mut left), buffer(&mut right)]);
        call_io_proc(&playback, list.as_mut_ptr());

        assert_eq!(left, vec![0x10, 0x11, 0x12, 0x13, 0, 0, 0, 0]);
        assert_eq!(right, vec![0x20, 0x21, 0x22, 0x23, 0, 0, 0, 0]);
        assert_eq!(status.underruns(), 1);
        assert!(!playback.finished.load(Ordering::Acquire));
        // need (4 frames pedidos) != got (2 frames lidos) de propósito: só assim a contagem
        // consegue provar que segue o pedido, e não o lido, tem como reprovar sob mutação.
        assert!((status.elapsed_seconds() - 0.04).abs() < 1e-9);
    }

    #[test]
    fn ramo_nao_intercalado_marca_fim_sem_contar_underrun_quando_produtora_terminou() {
        let status = Arc::new(SharedStatus::new());
        let client = client_format(4, 2);
        let playback = Playback::new(64, &client, true, Arc::clone(&status));
        playback.producer_done.store(true, Ordering::Release);
        // ring vazio de propósito: é o fim real da faixa, não uma falha de alimentação

        let mut left = vec![0xFFu8; 4]; // device pede 2 frames
        let mut right = vec![0xFFu8; 4];
        let mut list = SyntheticBufferList::new(&[buffer(&mut left), buffer(&mut right)]);
        call_io_proc(&playback, list.as_mut_ptr());

        assert!(left.iter().all(|&b| b == 0));
        assert!(right.iter().all(|&b| b == 0));
        assert!(playback.finished.load(Ordering::Acquire));
        assert_eq!(status.underruns(), 0);
    }
}
