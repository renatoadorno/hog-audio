#include "hog_device.hpp"

#include <unistd.h>

#include <chrono>
#include <cmath>
#include <thread>

#include "audio_source.hpp"  // osStatusText

namespace hog {
namespace {

AudioObjectPropertyAddress at(AudioObjectPropertySelector selector,
                              AudioObjectPropertyScope scope = kAudioObjectPropertyScopeGlobal) {
    return {selector, scope, kAudioObjectPropertyElementMain};
}

template <typename T>
OSStatus getValue(AudioObjectID object, const AudioObjectPropertyAddress& address, T& value) {
    UInt32 size = sizeof(T);
    return AudioObjectGetPropertyData(object, &address, 0, nullptr, &size, &value);
}

template <typename T>
OSStatus setValue(AudioObjectID object, const AudioObjectPropertyAddress& address,
                  const T& value) {
    return AudioObjectSetPropertyData(object, &address, 0, nullptr, sizeof(T), &value);
}

template <typename T>
OSStatus getArray(AudioObjectID object, const AudioObjectPropertyAddress& address,
                  std::vector<T>& out) {
    UInt32 size = 0;
    OSStatus status = AudioObjectGetPropertyDataSize(object, &address, 0, nullptr, &size);
    if (status != noErr) return status;
    out.resize(size / sizeof(T));
    if (out.empty()) return noErr;
    return AudioObjectGetPropertyData(object, &address, 0, nullptr, &size, out.data());
}

std::string cfStringToStd(CFStringRef ref) {
    if (ref == nullptr) return {};
    char buffer[512] = {};
    CFStringGetCString(ref, buffer, sizeof(buffer), kCFStringEncodingUTF8);
    return buffer;
}

bool sameRate(double a, double b) {
    return std::fabs(a - b) < 0.5;
}

// O HAL aceita a troca de rate de forma assíncrona: o set retorna sucesso antes de o
// hardware assumir. Configurar o formato físico antes disso escreveria sobre um estado que
// ainda vai mudar, então esperamos o device confirmar.
bool waitForRate(AudioObjectID device, double target) {
    for (int i = 0; i < 200; ++i) {
        double current = 0;
        if (getValue(device, at(kAudioDevicePropertyNominalSampleRate), current) == noErr &&
            sameRate(current, target)) {
            return true;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    return false;
}

}  // namespace

std::string queryDefaultOutputDevice(OutputDevice& out) {
    OSStatus status =
        getValue(kAudioObjectSystemObject, at(kAudioHardwarePropertyDefaultOutputDevice), out.id);
    if (status != noErr || out.id == kAudioObjectUnknown) {
        return "não encontrei o device de saída padrão: " + osStatusText(status);
    }

    CFStringRef name = nullptr;
    if (getValue(out.id, at(kAudioObjectPropertyName), name) == noErr) {
        out.name = cfStringToStd(name);
        CFRelease(name);
    }

    std::vector<AudioStreamID> streams;
    status = getArray(out.id, at(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput),
                      streams);
    if (status != noErr || streams.empty()) {
        return "o device não expõe stream de saída: " + osStatusText(status);
    }
    out.streamId = streams.front();

    std::vector<AudioValueRange> rates;
    status = getArray(out.id, at(kAudioDevicePropertyAvailableNominalSampleRates), rates);
    if (status != noErr) return "não consegui listar os sample rates: " + osStatusText(status);
    for (const auto& r : rates) out.caps.rates.push_back({r.mMinimum, r.mMaximum});

    std::vector<AudioStreamRangedDescription> formats;
    status = getArray(out.streamId, at(kAudioStreamPropertyAvailablePhysicalFormats), formats);
    if (status != noErr) return "não consegui listar os formatos físicos: " + osStatusText(status);

    for (const auto& ranged : formats) {
        const AudioStreamBasicDescription& f = ranged.mFormat;
        if (f.mFormatID != kAudioFormatLinearPCM) continue;  // ignora passthrough tipo AC-3

        PhysicalFormatDesc desc{};
        desc.index = static_cast<int>(out.physicalFormats.size());
        desc.sampleRate = f.mSampleRate > 0 ? f.mSampleRate : ranged.mSampleRateRange.mMinimum;
        desc.sampleType =
            (f.mFormatFlags & kAudioFormatFlagIsFloat) ? SampleType::Float : SampleType::Integer;
        desc.bitsPerChannel = f.mBitsPerChannel;
        desc.channels = f.mChannelsPerFrame;

        out.caps.physicalFormats.push_back(desc);
        out.physicalFormats.push_back(f);
    }

    AudioStreamBasicDescription current{};
    if (getValue(out.streamId, at(kAudioStreamPropertyVirtualFormat), current) == noErr) {
        out.caps.outputChannels = current.mChannelsPerFrame;
    }

    getValue(out.id, at(kAudioDevicePropertyNominalSampleRate), out.nominalRate);
    if (getValue(out.id, at(kAudioDevicePropertyVolumeScalar, kAudioObjectPropertyScopeOutput),
                 out.volume) != noErr) {
        out.volume = -1;
    }

    return {};
}

HoggedDevice::~HoggedDevice() {
    restore();
}

std::string HoggedDevice::acquire(const OutputDevice& device, double rate,
                                  const AudioStreamBasicDescription& physical) {
    deviceId_ = device.id;
    streamId_ = device.streamId;

    originalRate_ = device.nominalRate;
    if (getValue(streamId_, at(kAudioStreamPropertyPhysicalFormat), originalPhysical_) != noErr ||
        getValue(streamId_, at(kAudioStreamPropertyVirtualFormat), originalVirtual_) != noErr) {
        return "não consegui ler o formato atual do device para poder restaurá-lo depois";
    }
    savedFormats_ = true;

    // O hog vem antes de mexer no formato: só o dono exclusivo troca o formato físico de
    // maneira confiável, e é ele que impede o mixer de somar outros clientes ao stream.
    const pid_t me = getpid();
    OSStatus status = setValue(deviceId_, at(kAudioDevicePropertyHogMode), me);
    if (status != noErr) {
        return "não consegui tomar o device em modo exclusivo: " + osStatusText(status);
    }
    pid_t owner = -1;
    getValue(deviceId_, at(kAudioDevicePropertyHogMode), owner);
    if (owner != me) {
        return "o device já está tomado pelo processo " + std::to_string(owner);
    }
    hogged_ = true;

    if (!sameRate(device.nominalRate, rate)) {
        status = setValue(deviceId_, at(kAudioDevicePropertyNominalSampleRate), rate);
        if (status != noErr) return "o device recusou o sample rate: " + osStatusText(status);
        if (!waitForRate(deviceId_, rate)) {
            return "o device não assumiu o sample rate pedido dentro do tempo esperado";
        }
    }

    status = setValue(streamId_, at(kAudioStreamPropertyPhysicalFormat), physical);
    if (status != noErr) return "o device recusou o formato físico: " + osStatusText(status);

    // O ideal é o IOProc ver exatamente o formato físico. Muitos devices recusam e mantêm
    // float32 no formato virtual — o que ainda preserva os bits, porque a conversão de
    // inteiro de até 24 bits para float32 e de volta é exata.
    virtualFormatLocked_ =
        setValue(streamId_, at(kAudioStreamPropertyVirtualFormat), physical) == noErr;

    if (getValue(streamId_, at(kAudioStreamPropertyVirtualFormat), streamFormat_) != noErr) {
        return "não consegui ler o formato que o device vai entregar ao callback";
    }
    return {};
}

std::string HoggedDevice::start(AudioDeviceIOProc proc, void* context) {
    OSStatus status = AudioDeviceCreateIOProcID(deviceId_, proc, context, &procId_);
    if (status != noErr || procId_ == nullptr) {
        return "não consegui registrar o callback de áudio: " + osStatusText(status);
    }
    status = AudioDeviceStart(deviceId_, procId_);
    if (status != noErr) return "não consegui iniciar a reprodução: " + osStatusText(status);
    running_ = true;
    return {};
}

void HoggedDevice::stop() {
    if (!running_) return;
    AudioDeviceStop(deviceId_, procId_);
    running_ = false;
}

void HoggedDevice::restore() {
    if (deviceId_ == kAudioObjectUnknown) return;

    stop();
    if (procId_ != nullptr) {
        AudioDeviceDestroyIOProcID(deviceId_, procId_);
        procId_ = nullptr;
    }
    if (savedFormats_) {
        setValue(streamId_, at(kAudioStreamPropertyVirtualFormat), originalVirtual_);
        setValue(streamId_, at(kAudioStreamPropertyPhysicalFormat), originalPhysical_);
        setValue(deviceId_, at(kAudioDevicePropertyNominalSampleRate), originalRate_);
        savedFormats_ = false;
    }
    if (hogged_) {
        const pid_t release = -1;
        setValue(deviceId_, at(kAudioDevicePropertyHogMode), release);
        hogged_ = false;
    }
}

}  // namespace hog
