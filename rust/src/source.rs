//! Lê frames de um arquivo usando o decodificador do próprio macOS — FLAC, ALAC, WAV, AIFF,
//! CAF, AAC e MP3 sem biblioteca externa. O formato de entrega é configurável para casar
//! exatamente com o que o device espera, de modo que nenhuma conversão aconteça fora daqui.

use coreaudio_sys::*;

use crate::ffi::os_status_text;
use crate::format::{FileFormat, Resolution};

pub struct AudioSource {
    file: ExtAudioFileRef,
    format: FileFormat,
    total_frames: i64,
    codec_name: String,
    client_bytes_per_frame: u32,
}

// O ExtAudioFileRef é um ponteiro opaco, que o Rust trata como não-enviável por precaução.
// Movê-lo para a thread produtora é seguro porque só ela o usa, e sequencialmente: o
// AudioSource nunca é compartilhado entre threads, apenas transferido para uma.
unsafe impl Send for AudioSource {}

// Formatos comprimidos (FLAC, ALAC) deixam mBitsPerChannel zerado e codificam a profundidade
// da fonte nos format flags, com a mesma convenção do Apple Lossless.
//
// O `allow` cobre os nomes das constantes, que vêm do cabeçalho da Apple e não seguem a
// convenção do Rust. Renomeá-las localmente esconderia a origem — é melhor casar com o nome
// que está na documentação do Core Audio.
#[allow(non_upper_case_globals)]
fn bit_depth_from_source_flags(flags: u32) -> u32 {
    match flags {
        kAppleLosslessFormatFlag_16BitSourceData => 16,
        kAppleLosslessFormatFlag_20BitSourceData => 20,
        kAppleLosslessFormatFlag_24BitSourceData => 24,
        kAppleLosslessFormatFlag_32BitSourceData => 32,
        _ => 0,
    }
}

fn four_cc(value: u32) -> String {
    let chars = value.to_be_bytes();
    if !chars.iter().all(|c| c.is_ascii_graphic() || *c == b' ') {
        return format!("{value}");
    }
    // Os códigos da Apple são `'aac '` e `'.mp3'`; mensagens e tela querem "aac" e "mp3".
    String::from_utf8_lossy(&chars)
        .trim_end()
        .trim_start_matches('.')
        .to_string()
}

/// A profundidade que o arquivo declara, ou 0 quando ele não tem uma. FLAC e ALAC deixam
/// `mBitsPerChannel` zerado e a guardam nos format flags; noutros codecs os flags significam
/// outra coisa (no AAC, o tipo de objeto MPEG-4) e não podem ser lidos assim.
#[allow(non_upper_case_globals)]
fn declared_bit_depth(format_id: u32, flags: u32, bits_per_channel: u32) -> u32 {
    if bits_per_channel != 0 {
        return bits_per_channel;
    }
    match format_id {
        kAudioFormatAppleLossless | kAudioFormatFLAC => bit_depth_from_source_flags(flags),
        _ => 0,
    }
}

/// Os codecs com perda que tocam com a saída do decodificador intacta (ADR 0002 do core). O
/// HE-AAC fica fora: o rate que ele declara pode ser o do núcleo, e o `ExtAudioFile`
/// resamplearia em silêncio.
fn is_supported_lossy(format_id: u32) -> bool {
    format_id == kAudioFormatMPEG4AAC || format_id == kAudioFormatMPEGLayer3
}

/// Bit/s em kbps, arredondado. Zero vira `None`: a tela mostra só o codec, nunca "0 kbps".
fn to_kbps(bits_per_second: u32) -> Option<u32> {
    let kbps = bits_per_second.saturating_add(500) / 1000;
    (kbps > 0).then_some(kbps)
}

/// O bitrate médio que o `AudioFile` por baixo do `ExtAudioFile` publica. `None` quando a
/// leitura falha: a faixa toca do mesmo jeito.
fn read_bitrate_kbps(file: ExtAudioFileRef) -> Option<u32> {
    // O `AudioFileID` pertence ao `ExtAudioFile`: não se fecha aqui.
    let mut audio_file: AudioFileID = std::ptr::null_mut();
    let mut size = std::mem::size_of::<AudioFileID>() as u32;
    let status = unsafe {
        ExtAudioFileGetProperty(
            file,
            kExtAudioFileProperty_AudioFile,
            &mut size,
            &mut audio_file as *mut _ as *mut _,
        )
    };
    if status != 0 || audio_file.is_null() {
        return None;
    }

    let mut bits_per_second: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        AudioFileGetProperty(
            audio_file,
            kAudioFilePropertyBitRate,
            &mut size,
            &mut bits_per_second as *mut _ as *mut _,
        )
    };
    if status != 0 {
        return None;
    }
    to_kbps(bits_per_second)
}

impl AudioSource {
    /// Devolve mensagem de erro; `Ok` em caso de sucesso.
    pub fn open(path: &str) -> Result<Self, String> {
        let bytes = path.as_bytes();
        let url = unsafe {
            CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                bytes.as_ptr(),
                bytes.len() as CFIndex,
                false as Boolean,
            )
        };
        if url.is_null() {
            return Err(format!("caminho inválido: {path}"));
        }

        let mut file: ExtAudioFileRef = std::ptr::null_mut();
        let status = unsafe { ExtAudioFileOpenURL(url, &mut file) };
        unsafe { CFRelease(url as *const _) };
        if status != 0 {
            return Err(format!(
                "não consegui abrir o arquivo: {}",
                os_status_text(status)
            ));
        }

        let mut asbd = AudioStreamBasicDescription::default();
        let mut size = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
        let status = unsafe {
            ExtAudioFileGetProperty(
                file,
                kExtAudioFileProperty_FileDataFormat,
                &mut size,
                &mut asbd as *mut _ as *mut _,
            )
        };
        if status != 0 {
            unsafe { ExtAudioFileDispose(file) };
            return Err(format!(
                "não consegui ler o formato do arquivo: {}",
                os_status_text(status)
            ));
        }

        let codec_name = four_cc(asbd.mFormatID);
        let bit_depth = declared_bit_depth(asbd.mFormatID, asbd.mFormatFlags, asbd.mBitsPerChannel);
        let resolution = if bit_depth != 0 {
            Resolution::Lossless { bits: bit_depth }
        } else if is_supported_lossy(asbd.mFormatID) {
            Resolution::Lossy {
                bitrate_kbps: read_bitrate_kbps(file),
            }
        } else {
            unsafe { ExtAudioFileDispose(file) };
            return Err(format!(
                "{codec_name}: sem profundidade de bits e fora dos codecs com perda suportados \
                 (AAC-LC e MP3)"
            ));
        };

        let mut total_frames: i64 = 0;
        let mut size = std::mem::size_of::<i64>() as u32;
        let status = unsafe {
            ExtAudioFileGetProperty(
                file,
                kExtAudioFileProperty_FileLengthFrames,
                &mut size,
                &mut total_frames as *mut _ as *mut _,
            )
        };
        if status != 0 {
            unsafe { ExtAudioFileDispose(file) };
            return Err(format!(
                "não consegui ler a duração: {}",
                os_status_text(status)
            ));
        }

        Ok(Self {
            file,
            format: FileFormat {
                sample_rate: asbd.mSampleRate,
                resolution,
                channels: asbd.mChannelsPerFrame,
            },
            total_frames,
            codec_name,
            client_bytes_per_frame: 0,
        })
    }

    pub fn format(&self) -> FileFormat {
        self.format
    }

    pub fn total_frames(&self) -> i64 {
        self.total_frames
    }

    pub fn codec_name(&self) -> &str {
        &self.codec_name
    }

    /// Formato em que `read` entregará os frames. Precisa ser PCM.
    pub fn set_client_format(&mut self, asbd: &AudioStreamBasicDescription) -> Result<(), String> {
        let status = unsafe {
            ExtAudioFileSetProperty(
                self.file,
                kExtAudioFileProperty_ClientDataFormat,
                std::mem::size_of::<AudioStreamBasicDescription>() as u32,
                asbd as *const _ as *const _,
            )
        };
        if status != 0 {
            return Err(format!(
                "o decodificador recusou o formato de entrega: {}",
                os_status_text(status)
            ));
        }
        self.client_bytes_per_frame = asbd.mBytesPerFrame;
        Ok(())
    }

    /// O formato que o decodificador de fato assumiu, relido dele. Pode divergir do que foi
    /// pedido, e é esse que descreve os bytes que `read` vai produzir.
    pub fn effective_client_format(&self) -> Result<AudioStreamBasicDescription, String> {
        let mut asbd = AudioStreamBasicDescription::default();
        let mut size = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
        let status = unsafe {
            ExtAudioFileGetProperty(
                self.file,
                kExtAudioFileProperty_ClientDataFormat,
                &mut size,
                &mut asbd as *mut _ as *mut _,
            )
        };
        if status != 0 {
            return Err(format!(
                "não consegui confirmar o formato de entrega: {}",
                os_status_text(status)
            ));
        }
        Ok(asbd)
    }

    /// Lê até `frames` para dentro de `dst`. Devolve quantos frames leu; 0 significa fim.
    pub fn read(&mut self, dst: &mut [u8], frames: u32) -> u32 {
        if self.client_bytes_per_frame == 0 {
            return 0; // set_client_format ainda não rodou
        }
        // mDataByteSize é UInt32: um pedido grande demais transbordaria e descreveria um
        // buffer menor do que o que seria escrito.
        let max_frames = u32::MAX / self.client_bytes_per_frame;
        let capacity_frames = (dst.len() / self.client_bytes_per_frame as usize) as u32;
        let frames = frames.min(max_frames).min(capacity_frames);
        if frames == 0 {
            return 0;
        }

        let mut list = AudioBufferList {
            mNumberBuffers: 1,
            mBuffers: [AudioBuffer {
                mNumberChannels: 0, // irrelevante para dados intercalados
                mDataByteSize: frames * self.client_bytes_per_frame,
                mData: dst.as_mut_ptr() as *mut _,
            }],
        };

        let mut frames_read = frames;
        let status = unsafe { ExtAudioFileRead(self.file, &mut frames_read, &mut list) };
        if status != 0 {
            return 0;
        }
        frames_read
    }
}

impl Drop for AudioSource {
    fn drop(&mut self) {
        if !self.file.is_null() {
            unsafe { ExtAudioFileDispose(self.file) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_formato_do_flac_de_teste() {
        let path = crate::fixtures::path("t96_24.flac");
        let source = AudioSource::open(&path).expect("deveria abrir o arquivo");
        let f = source.format();

        assert_eq!(f.sample_rate, 96000.0);
        assert_eq!(f.resolution, Resolution::Lossless { bits: 24 });
        assert_eq!(f.channels, 2);
        assert_eq!(source.codec_name(), "flac");
        assert!(source.total_frames() > 0);
    }

    #[test]
    fn arquivo_inexistente_devolve_erro() {
        assert!(AudioSource::open("/tmp/nao-existe-mesmo.flac").is_err());
    }

    #[test]
    fn codigo_do_codec_sai_sem_espaco_e_sem_ponto() {
        assert_eq!(four_cc(kAudioFormatMPEG4AAC), "aac");
        assert_eq!(four_cc(kAudioFormatMPEGLayer3), "mp3");
        assert_eq!(four_cc(kAudioFormatMPEG4AAC_HE), "aach");
        assert_eq!(four_cc(kAudioFormatFLAC), "flac");
        assert_eq!(four_cc(1), "1");
    }

    // No AAC os format flags guardam o tipo de objeto MPEG-4 (2 = LC), que coincide com o flag
    // de 20 bits do ALAC: lidos como profundidade, um AAC viraria "20 bits sem perda".
    #[test]
    fn flags_do_aac_nao_viram_profundidade_de_bits() {
        assert_eq!(declared_bit_depth(kAudioFormatMPEG4AAC, 2, 0), 0);
        assert_eq!(declared_bit_depth(kAudioFormatMPEGLayer3, 0, 0), 0);
    }

    #[test]
    fn flac_e_alac_leem_a_profundidade_dos_flags() {
        assert_eq!(
            declared_bit_depth(
                kAudioFormatFLAC,
                kAppleLosslessFormatFlag_24BitSourceData,
                0
            ),
            24
        );
        assert_eq!(
            declared_bit_depth(
                kAudioFormatAppleLossless,
                kAppleLosslessFormatFlag_16BitSourceData,
                0
            ),
            16
        );
        assert_eq!(declared_bit_depth(kAudioFormatLinearPCM, 0, 16), 16);
    }

    #[test]
    fn bitrate_que_arredonda_para_zero_vira_ausente() {
        assert_eq!(to_kbps(0), None);
        assert_eq!(to_kbps(499), None);
        assert_eq!(to_kbps(339_163), Some(339));
        assert_eq!(to_kbps(191_998), Some(192));
        assert_eq!(to_kbps(u32::MAX), Some(4_294_967));
    }

    #[test]
    fn aac_lc_abre_como_faixa_com_perda_com_bitrate() {
        let source = AudioSource::open(&crate::fixtures::path("t44_aac.m4a"))
            .expect("deveria abrir o AAC-LC");
        let f = source.format();

        assert_eq!(source.codec_name(), "aac");
        assert_eq!(f.sample_rate, 44100.0);
        assert_eq!(f.channels, 2);
        let Resolution::Lossy {
            bitrate_kbps: Some(kbps),
        } = f.resolution
        else {
            panic!("deveria ser com perda e com bitrate: {:?}", f.resolution);
        };
        // Faixa, e não valor exato: o AAC sai em VBR, e o bitrate depende do encoder da Apple.
        assert!(kbps > 0 && kbps <= 400, "{kbps}");
        let seconds = source.total_frames() as f64 / f.sample_rate;
        assert!((seconds - 6.0).abs() < 0.05, "{seconds}");
    }

    #[test]
    fn mp3_abre_como_faixa_com_perda_com_bitrate() {
        let source =
            AudioSource::open(&crate::fixtures::path("t44_mp3.mp3")).expect("deveria abrir o MP3");

        assert_eq!(source.codec_name(), "mp3");
        let Resolution::Lossy {
            bitrate_kbps: Some(kbps),
        } = source.format().resolution
        else {
            panic!("deveria ser com perda e com bitrate");
        };
        assert!((190..=194).contains(&kbps), "{kbps}");
    }

    #[test]
    fn he_aac_e_recusado_com_o_nome_do_codec() {
        let Err(erro) = AudioSource::open(&crate::fixtures::path("t44_heaac.m4a")) else {
            panic!("o HE-AAC fica fora por enquanto");
        };

        assert_eq!(
            erro,
            "aach: sem profundidade de bits e fora dos codecs com perda suportados \
             (AAC-LC e MP3)"
        );
    }

    #[test]
    fn alac_em_m4a_continua_sem_perda() {
        let source = AudioSource::open(&crate::fixtures::path("t44_alac.m4a"))
            .expect("deveria abrir o ALAC");

        assert_eq!(source.codec_name(), "alac");
        assert_eq!(
            source.format().resolution,
            Resolution::Lossless { bits: 16 }
        );
    }
}
