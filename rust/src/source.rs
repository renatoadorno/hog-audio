//! Lê frames de um arquivo usando o decodificador do próprio macOS — FLAC, ALAC, WAV, AIFF,
//! CAF, AAC e MP3 sem biblioteca externa. O formato de entrega é configurável para casar
//! exatamente com o que o device espera, de modo que nenhuma conversão aconteça fora daqui.

use coreaudio_sys::*;

use crate::ffi::os_status_text;
use crate::format::FileFormat;

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
    let chars = [
        ((value >> 24) & 0xFF) as u8,
        ((value >> 16) & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,
    ];
    if chars.iter().all(|c| c.is_ascii_graphic()) {
        String::from_utf8_lossy(&chars).into_owned()
    } else {
        format!("{value}")
    }
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

        let bit_depth = if asbd.mBitsPerChannel != 0 {
            asbd.mBitsPerChannel
        } else {
            bit_depth_from_source_flags(asbd.mFormatFlags)
        };
        let codec_name = four_cc(asbd.mFormatID);

        if bit_depth == 0 {
            unsafe { ExtAudioFileDispose(file) };
            return Err(format!(
                "não consegui determinar a profundidade de bits de {codec_name}; \
                 sem isso não dá para garantir reprodução sem perda"
            ));
        }

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
                bit_depth,
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
        assert_eq!(f.bit_depth, 24);
        assert_eq!(f.channels, 2);
        assert_eq!(source.codec_name(), "flac");
        assert!(source.total_frames() > 0);
    }

    #[test]
    fn arquivo_inexistente_devolve_erro() {
        assert!(AudioSource::open("/tmp/nao-existe-mesmo.flac").is_err());
    }
}
