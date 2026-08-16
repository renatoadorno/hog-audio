#include <csignal>
#include <cstring>
#include <chrono>
#include <cstdio>
#include <string>
#include <thread>
#include <vector>

#include "audio_source.hpp"
#include "format_negotiation.hpp"
#include "hog_device.hpp"
#include "ring_buffer.hpp"

namespace {

volatile std::sig_atomic_t g_interrupted = 0;

void onInterrupt(int) {
    g_interrupted = 1;  // a limpeza é do main: o handler não pode chamar Core Audio
}

// Estado compartilhado entre a thread que decodifica e o IOProc de tempo real.
struct Playback {
    explicit Playback(std::size_t ringBytes) : ring(ringBytes) {}

    hog::RingBuffer ring;
    std::atomic<bool> producerDone{false};
    std::atomic<bool> finished{false};
    std::atomic<std::uint64_t> underruns{0};

    std::uint32_t bytesPerFrame = 0;   // sempre intercalado, como está no ring buffer
    std::uint32_t bytesPerSample = 0;
    bool nonInterleaved = false;
    std::vector<std::uint8_t> scratch;  // pré-alocado: o IOProc não pode alocar
};

// Roda em thread de tempo real: só memcpy e atomics. Nada de alocar, travar ou imprimir.
OSStatus ioProc(AudioObjectID, const AudioTimeStamp*, const AudioBufferList*,
                const AudioTimeStamp*, AudioBufferList* outputData, const AudioTimeStamp*,
                void* context) {
    auto* p = static_cast<Playback*>(context);

    if (!p->nonInterleaved) {
        AudioBuffer& buffer = outputData->mBuffers[0];
        const std::size_t need = buffer.mDataByteSize;
        const std::size_t got = p->ring.read(buffer.mData, need);
        if (got < need) std::memset(static_cast<std::uint8_t*>(buffer.mData) + got, 0, need - got);
        if (got < need && p->producerDone.load(std::memory_order_acquire)) {
            p->finished.store(true, std::memory_order_release);
        } else if (got < need) {
            p->underruns.fetch_add(1, std::memory_order_relaxed);
        }
        return noErr;
    }

    const std::uint32_t frames = outputData->mBuffers[0].mDataByteSize / p->bytesPerSample;
    const std::size_t need = static_cast<std::size_t>(frames) * p->bytesPerFrame;
    const std::size_t got = p->ring.read(p->scratch.data(), need);
    if (got < need) std::memset(p->scratch.data() + got, 0, need - got);

    // Desintercala: o ring guarda LRLRLR..., o device quer um buffer por canal.
    for (UInt32 ch = 0; ch < outputData->mNumberBuffers; ++ch) {
        auto* dst = static_cast<std::uint8_t*>(outputData->mBuffers[ch].mData);
        const std::uint8_t* src = p->scratch.data() + ch * p->bytesPerSample;
        for (std::uint32_t f = 0; f < frames; ++f) {
            std::memcpy(dst + f * p->bytesPerSample, src + f * p->bytesPerFrame,
                        p->bytesPerSample);
        }
    }

    if (got < need && p->producerDone.load(std::memory_order_acquire)) {
        p->finished.store(true, std::memory_order_release);
    } else if (got < need) {
        p->underruns.fetch_add(1, std::memory_order_relaxed);
    }
    return noErr;
}

std::string describeFormat(const AudioStreamBasicDescription& f) {
    std::string type = (f.mFormatFlags & kAudioFormatFlagIsFloat) ? "float" : "inteiro";
    std::string layout =
        (f.mFormatFlags & kAudioFormatFlagIsNonInterleaved) ? ", não intercalado" : "";
    return std::to_string(static_cast<long long>(f.mSampleRate)) + " Hz / " +
           std::to_string(f.mBitsPerChannel) + " bits " + type + " / " +
           std::to_string(f.mChannelsPerFrame) + " canais" + layout;
}

std::string describeSource(const hog::AudioSource& source) {
    const hog::FileFormat& f = source.format();
    return source.codecName() + " — " + std::to_string(static_cast<long long>(f.sampleRate)) +
           " Hz / " + std::to_string(f.bitDepth) + " bits / " + std::to_string(f.channels) +
           " canais";
}

void printDeviceReport(const hog::OutputDevice& device, const hog::AudioSource& source) {
    std::printf("arquivo  : %s\n", describeSource(source).c_str());
    std::printf("device   : %s\n", device.name.c_str());
    std::printf("rate atual: %lld Hz\n", static_cast<long long>(device.nominalRate));

    std::string rates;
    for (const auto& r : device.caps.rates) {
        if (!rates.empty()) rates += ", ";
        rates += std::to_string(static_cast<long long>(r.minimum));
        if (r.maximum != r.minimum) rates += "-" + std::to_string(static_cast<long long>(r.maximum));
    }
    std::printf("rates    : %s\n", rates.c_str());
    std::printf("formatos : %zu físicos disponíveis\n", device.physicalFormats.size());
    for (const auto& f : device.physicalFormats) {
        std::printf("           %s\n", describeFormat(f).c_str());
    }
    if (device.volume >= 0) std::printf("volume   : %.0f%%\n", device.volume * 100.0);
}

int usage() {
    std::fprintf(stderr,
                 "uso: hog-audio [--info] <arquivo>\n\n"
                 "  Reproduz o arquivo tomando o DAC em modo exclusivo, travado no sample\n"
                 "  rate e no bit depth do próprio arquivo. Ctrl+C interrompe.\n\n"
                 "  --info  mostra o que seria negociado, sem tocar no device\n");
    return 2;
}

}  // namespace

int main(int argc, char** argv) {
    // Sem isto, redirecionar a saída embaralha a ordem entre stdout e stderr.
    std::setvbuf(stdout, nullptr, _IOLBF, 0);

    std::string path;
    bool infoOnly = false;
    for (int i = 1; i < argc; ++i) {
        const std::string arg = argv[i];
        if (arg == "--info") {
            infoOnly = true;
        } else if (arg == "-h" || arg == "--help") {
            return usage();
        } else if (path.empty()) {
            path = arg;
        } else {
            return usage();
        }
    }
    if (path.empty()) return usage();

    hog::AudioSource source;
    if (std::string error = source.open(path); !error.empty()) {
        std::fprintf(stderr, "erro: %s\n", error.c_str());
        return 1;
    }

    hog::OutputDevice device;
    if (std::string error = hog::queryDefaultOutputDevice(device); !error.empty()) {
        std::fprintf(stderr, "erro: %s\n", error.c_str());
        return 1;
    }

    printDeviceReport(device, source);

    const hog::Decision decision = hog::negotiate(source.format(), device.caps);
    if (!decision.play) {
        std::fprintf(stderr, "\nnão dá para reproduzir sem perda: %s\n", decision.reason.c_str());
        return 1;
    }
    std::printf("negociado: %s\n", decision.reason.c_str());
    if (decision.duplicateMonoToStereo) {
        std::printf("aviso    : arquivo mono, será duplicado nos dois canais\n");
    }
    if (infoOnly) return 0;

    hog::HoggedDevice hogged;
    const AudioStreamBasicDescription& physical =
        device.physicalFormats[static_cast<std::size_t>(decision.physicalFormatIndex)];
    if (std::string error = hogged.acquire(device, decision.sampleRate, physical);
        !error.empty()) {
        std::fprintf(stderr, "erro: %s\n", error.c_str());
        return 1;
    }

    const AudioStreamBasicDescription& stream = hogged.streamFormat();
    std::printf("exclusivo: sim (hog mode)\n");
    std::printf("físico   : %s\n", describeFormat(physical).c_str());
    std::printf("callback : %s%s\n", describeFormat(stream).c_str(),
                hogged.virtualFormatLocked() ? " [travado igual ao físico]" : "");

    // O decodificador entrega sempre intercalado, que é como o ring buffer guarda; o IOProc
    // desintercala se o device pedir assim.
    AudioStreamBasicDescription client = stream;
    client.mFormatFlags &= ~static_cast<UInt32>(kAudioFormatFlagIsNonInterleaved);
    client.mFramesPerPacket = 1;
    client.mBytesPerFrame = client.mChannelsPerFrame * (client.mBitsPerChannel / 8);
    client.mBytesPerPacket = client.mBytesPerFrame;

    if (std::string error = source.setClientFormat(client); !error.empty()) {
        std::fprintf(stderr, "erro: %s\n", error.c_str());
        return 1;
    }

    const std::size_t ringBytes =
        static_cast<std::size_t>(client.mSampleRate) * client.mBytesPerFrame * 2;
    Playback playback(ringBytes);
    playback.bytesPerFrame = client.mBytesPerFrame;
    playback.bytesPerSample = client.mBitsPerChannel / 8;
    playback.nonInterleaved = (stream.mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;
    playback.scratch.resize(ringBytes);

    std::thread producer([&] {
        std::vector<std::uint8_t> chunk(64 * 1024);
        const std::uint32_t framesPerChunk =
            static_cast<std::uint32_t>(chunk.size() / client.mBytesPerFrame);
        while (g_interrupted == 0) {
            const std::uint32_t got = source.read(chunk.data(), framesPerChunk);
            if (got == 0) break;

            std::size_t written = 0;
            const std::size_t bytes = static_cast<std::size_t>(got) * client.mBytesPerFrame;
            while (written < bytes && g_interrupted == 0) {
                written += playback.ring.write(chunk.data() + written, bytes - written);
                if (written < bytes) std::this_thread::sleep_for(std::chrono::milliseconds(5));
            }
        }
        playback.producerDone.store(true, std::memory_order_release);
    });

    std::signal(SIGINT, onInterrupt);
    std::signal(SIGTERM, onInterrupt);

    if (std::string error = hogged.start(ioProc, &playback); !error.empty()) {
        std::fprintf(stderr, "erro: %s\n", error.c_str());
        g_interrupted = 1;
        producer.join();
        return 1;
    }

    const double seconds = static_cast<double>(source.totalFrames()) / source.format().sampleRate;
    std::printf("tocando  : %.1f s — Ctrl+C interrompe\n", seconds);

    while (g_interrupted == 0 && !playback.finished.load(std::memory_order_acquire)) {
        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }

    const bool interruptedByUser = g_interrupted != 0;

    hogged.stop();
    g_interrupted = 1;  // desbloqueia a thread produtora, que espera espaço no ring
    producer.join();

    const std::uint64_t underruns = playback.underruns.load(std::memory_order_relaxed);
    if (underruns > 0) {
        std::printf("aviso    : %llu falhas de alimentação do buffer\n",
                    static_cast<unsigned long long>(underruns));
    }
    std::printf("fim      : device restaurado\n");
    return interruptedByUser ? 130 : 0;  // 128 + SIGINT, como manda a convenção
}
