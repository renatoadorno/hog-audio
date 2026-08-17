#pragma once

#include <AudioToolbox/AudioToolbox.h>

#include <cstdint>
#include <string>

#include "format_negotiation.hpp"

namespace hog {

// Lê frames de um arquivo usando o decodificador do próprio macOS — FLAC, ALAC, WAV, AIFF,
// CAF, AAC e MP3 sem biblioteca externa. O formato de entrega é configurável para casar
// exatamente com o que o device espera, de modo que nenhuma conversão aconteça fora daqui.
class AudioSource {
public:
    AudioSource() = default;
    ~AudioSource();
    AudioSource(const AudioSource&) = delete;
    AudioSource& operator=(const AudioSource&) = delete;
    AudioSource(AudioSource&&) = delete;
    AudioSource& operator=(AudioSource&&) = delete;

    // Devolve mensagem de erro; string vazia em caso de sucesso.
    std::string open(const std::string& path);

    const FileFormat& format() const { return format_; }
    std::int64_t totalFrames() const { return totalFrames_; }
    const std::string& codecName() const { return codecName_; }

    // Formato em que read() entregará os frames. Precisa ser PCM.
    std::string setClientFormat(const AudioStreamBasicDescription& asbd);

    // O formato que o decodificador de fato assumiu, relido dele. Pode divergir do que foi
    // pedido, e é esse que descreve os bytes que read() vai produzir.
    std::string effectiveClientFormat(AudioStreamBasicDescription& asbd) const;

    // Lê até `frames`. Devolve quantos frames leu; 0 significa fim do arquivo.
    std::uint32_t read(void* dst, std::uint32_t frames);

private:
    ExtAudioFileRef file_ = nullptr;
    FileFormat format_{};
    std::int64_t totalFrames_ = 0;
    std::string codecName_;
    std::uint32_t clientBytesPerFrame_ = 0;
};

// Formata um OSStatus como fourcc legível quando possível, senão como número.
std::string osStatusText(OSStatus status);

}  // namespace hog
