#pragma once

#include <CoreAudio/CoreAudio.h>

#include <string>
#include <vector>

#include "format_negotiation.hpp"

namespace hog {

// O device de saída e tudo que ele aceita. `physicalFormats` é paralelo a
// `caps.physicalFormats`: o núcleo escolhe um índice, aqui está o descritor correspondente.
struct OutputDevice {
    AudioObjectID id = kAudioObjectUnknown;
    AudioObjectID streamId = kAudioObjectUnknown;
    std::string name;
    DeviceCaps caps;
    std::vector<AudioStreamBasicDescription> physicalFormats;
    double nominalRate = 0;
    float volume = -1;  // negativo quando o device não expõe volume
};

// Descreve o device de saída padrão. Devolve mensagem de erro; vazia em caso de sucesso.
std::string queryDefaultOutputDevice(OutputDevice& out);

// Toma o device para uso exclusivo e trava rate e formato físico. O destrutor devolve tudo
// ao estado anterior — inclusive quando a aquisição falha no meio do caminho.
class HoggedDevice {
public:
    HoggedDevice() = default;
    ~HoggedDevice();
    HoggedDevice(const HoggedDevice&) = delete;
    HoggedDevice& operator=(const HoggedDevice&) = delete;
    HoggedDevice(HoggedDevice&&) = delete;
    HoggedDevice& operator=(HoggedDevice&&) = delete;

    std::string acquire(const OutputDevice& device, double rate,
                        const AudioStreamBasicDescription& physical);

    // O formato que o IOProc vai receber. Igual ao físico quando o HAL aceitou travá-lo;
    // caso contrário, o formato virtual que o device impôs (tipicamente float32).
    const AudioStreamBasicDescription& streamFormat() const { return streamFormat_; }
    bool virtualFormatLocked() const { return virtualFormatLocked_; }

    std::string start(AudioDeviceIOProc proc, void* context);
    void stop();

    // Devolve o device ao estado original agora, em vez de esperar o destrutor, e informa o
    // que falhou. Sem isto não haveria como distinguir uma restauração bem-sucedida de uma
    // que deixou o device alterado. Chamar de novo (ou pelo destrutor) não repete o trabalho.
    std::string finish();

private:
    void restore();

    AudioObjectID deviceId_ = kAudioObjectUnknown;
    AudioObjectID streamId_ = kAudioObjectUnknown;
    AudioDeviceIOProcID procId_ = nullptr;
    bool running_ = false;
    bool hogged_ = false;

    AudioStreamBasicDescription streamFormat_{};
    bool virtualFormatLocked_ = false;

    std::string restoreError_;
    double originalRate_ = 0;
    AudioStreamBasicDescription originalPhysical_{};
    AudioStreamBasicDescription originalVirtual_{};
    bool savedFormats_ = false;
};

}  // namespace hog
