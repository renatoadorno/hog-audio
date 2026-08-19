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
