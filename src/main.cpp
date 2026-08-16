#include <chrono>
#include <cmath>
#include <csignal>
#include <cstdio>
#include <cstring>
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
    std::uint32_t bytesPerSample = 0;  // por canal, incluindo o padding do container
    std::uint32_t channels = 0;
    bool nonInterleaved = false;
    std::vector<std::uint8_t> scratch;  // pré-alocado: o IOProc não pode alocar
};

// Roda em thread de tempo real: só memcpy e atomics. Nada de alocar, travar ou imprimir.
OSStatus ioProc(AudioObjectID, const AudioTimeStamp*, const AudioBufferList*,
                const AudioTimeStamp*, AudioBufferList* outputData, const AudioTimeStamp*,
                void* context) {
    auto* p = static_cast<Playback*>(context);
    if (outputData == nullptr || outputData->mNumberBuffers == 0) return noErr;

    // Silêncio é o único preenchimento seguro: lixo de memória enviado ao DAC vira ruído
    // branco em volume total. Toda saída daqui é dado do arquivo ou zero, nunca outra coisa.
    auto silenceAll = [&] {
        for (UInt32 i = 0; i < outputData->mNumberBuffers; ++i) {
            std::memset(outputData->mBuffers[i].mData, 0, outputData->mBuffers[i].mDataByteSize);
        }
    };

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
    if (need > p->scratch.size()) {  // o device pediu mais do que reservamos: cala, não arrisca
        silenceAll();
        p->underruns.fetch_add(1, std::memory_order_relaxed);
        return noErr;
    }

    const std::size_t got = p->ring.read(p->scratch.data(), need);
    if (got < need) std::memset(p->scratch.data() + got, 0, need - got);

    // Desintercala: o ring guarda LRLRLR..., o device quer um buffer por canal. Buffers além
    // dos canais que temos recebem silêncio.
    const UInt32 channels = p->channels;
    for (UInt32 ch = 0; ch < outputData->mNumberBuffers; ++ch) {
        auto* dst = static_cast<std::uint8_t*>(outputData->mBuffers[ch].mData);
        if (ch >= channels) {
            std::memset(dst, 0, outputData->mBuffers[ch].mDataByteSize);
            continue;
        }
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

    if (stream.mFormatID != kAudioFormatLinearPCM) {
        std::fprintf(stderr, "erro: o device não está em PCM linear; não vou alimentá-lo\n");
        return 1;
    }

    // O decodificador entrega sempre intercalado, que é como o ring buffer guarda; o IOProc
    // desintercala se o device pedir assim.
    //
    // O tamanho do frame vem do próprio device, nunca de bitsPerChannel/8: um formato pode
    // carregar amostras de 24 bits em containers de 32, e recalcular assumindo empacotamento
    // faria o decodificador produzir um passo e o IOProc ler outro — ruído branco, não música.
    const bool nonInterleaved = (stream.mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;
    AudioStreamBasicDescription client = stream;
    client.mFormatFlags &= ~static_cast<UInt32>(kAudioFormatFlagIsNonInterleaved);
    client.mFramesPerPacket = 1;
    client.mBytesPerFrame =
        nonInterleaved ? stream.mBytesPerFrame * stream.mChannelsPerFrame : stream.mBytesPerFrame;
    client.mBytesPerPacket = client.mBytesPerFrame;

    if (hog::FormatCheck check = hog::validateInterleavedFormat(
            client.mBitsPerChannel, client.mBytesPerFrame, client.mChannelsPerFrame);
        !check.ok) {
        std::fprintf(stderr, "erro: formato de entrega inconsistente: %s\n", check.reason.c_str());
        return 1;
    }

    if (std::string error = source.setClientFormat(client); !error.empty()) {
        std::fprintf(stderr, "erro: %s\n", error.c_str());
        return 1;
    }

    // O decodificador pode ajustar o que aceitou. Se o que ele vai entregar divergir do que
    // o device espera, parar aqui é a diferença entre silêncio e ruído em volume total.
    AudioStreamBasicDescription effective{};
    if (std::string error = source.effectiveClientFormat(effective); !error.empty()) {
        std::fprintf(stderr, "erro: %s\n", error.c_str());
        return 1;
    }
    if (effective.mBitsPerChannel != client.mBitsPerChannel ||
        effective.mBytesPerFrame != client.mBytesPerFrame ||
        effective.mChannelsPerFrame != client.mChannelsPerFrame ||
        (effective.mFormatFlags & kAudioFormatFlagIsFloat) !=
            (client.mFormatFlags & kAudioFormatFlagIsFloat) ||
        std::fabs(effective.mSampleRate - client.mSampleRate) > 0.5) {
        std::fprintf(stderr,
                     "erro: o decodificador vai entregar %s, mas o device espera %s; "
                     "reproduzir assim geraria ruído\n",
                     describeFormat(effective).c_str(), describeFormat(client).c_str());
        return 1;
    }

    const std::size_t ringBytes =
        static_cast<std::size_t>(client.mSampleRate) * client.mBytesPerFrame * 2;
    Playback playback(ringBytes);
    playback.bytesPerFrame = client.mBytesPerFrame;
    playback.bytesPerSample = client.mBytesPerFrame / client.mChannelsPerFrame;
    playback.channels = client.mChannelsPerFrame;
    playback.nonInterleaved = nonInterleaved;
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

    // Uma std::thread ainda unível no destrutor chama std::terminate, e aí nada restaura o
    // device. Este guard encerra e une a produtora em qualquer saída, inclusive por exceção,
    // garantindo que o destrutor de HoggedDevice chegue a rodar.
    struct ProducerGuard {
        std::thread& thread;
        ~ProducerGuard() {
            g_interrupted = 1;
            if (thread.joinable()) thread.join();
        }
    } producerGuard{producer};

    // SIGHUP cobre o terminal sendo fechado e SIGQUIT o Ctrl+\; sem eles o processo morreria
    // sem devolver o device, deixando o sample rate trocado.
    for (int sig : {SIGINT, SIGTERM, SIGHUP, SIGQUIT}) std::signal(sig, onInterrupt);

    // Deixa o buffer encher antes de abrir o fluxo, para o começo da faixa não sair picotado.
    for (int i = 0; i < 200 && playback.ring.availableToRead() < ringBytes / 2 &&
                    !playback.producerDone.load(std::memory_order_acquire) && g_interrupted == 0;
         ++i) {
        std::this_thread::sleep_for(std::chrono::milliseconds(5));
    }

    if (std::string error = hogged.start(ioProc, &playback); !error.empty()) {
        std::fprintf(stderr, "erro: %s\n", error.c_str());
        return 1;  // producerGuard encerra a thread; o destrutor de hogged devolve o device
    }

    const double seconds = static_cast<double>(source.totalFrames()) / source.format().sampleRate;
    std::printf("tocando  : %.1f s — Ctrl+C interrompe\n", seconds);

    while (g_interrupted == 0 && !playback.finished.load(std::memory_order_acquire)) {
        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }

    const bool interruptedByUser = g_interrupted != 0;

    hogged.stop();
    g_interrupted = 1;  // desbloqueia a thread produtora, que espera espaço no ring
    producer.join();    // aqui é o caminho normal; o guard cobre os caminhos de exceção

    const std::uint64_t underruns = playback.underruns.load(std::memory_order_relaxed);
    if (underruns > 0) {
        std::printf("aviso    : %llu falhas de alimentação do buffer\n",
                    static_cast<unsigned long long>(underruns));
    }
    std::printf("fim      : device restaurado\n");
    return interruptedByUser ? 130 : 0;  // 128 + SIGINT, como manda a convenção
}
