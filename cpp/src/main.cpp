#include <atomic>
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
#include "volume.hpp"

namespace {

// Atômico, não apenas volatile: além do handler de sinal, a thread produtora e a principal
// leem esta flag, e volatile não garante visibilidade entre threads. std::atomic<int> é
// lock-free nesta plataforma, portanto continua seguro dentro de um handler.
std::atomic<int> g_interrupted{0};

void onInterrupt(int) {
    g_interrupted.store(1, std::memory_order_relaxed);  // a limpeza é do main
}

bool interrupted() {
    return g_interrupted.load(std::memory_order_relaxed) != 0;
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
        const std::size_t take =
            hog::alignedReadSize(need, p->ring.availableToRead(), p->bytesPerFrame);
        const std::size_t got = p->ring.read(buffer.mData, take);
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

    const std::size_t take =
        hog::alignedReadSize(need, p->ring.availableToRead(), p->bytesPerFrame);
    const std::size_t got = p->ring.read(p->scratch.data(), take);
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

constexpr double kDefaultCeiling = 0.5;

// Descreve o volume que o device realmente assumiu, lido do hardware. A conversão de escalar
// para decibéis publicada pelo HAL não bate com a curva aplicada de fato, então exibir o
// valor convertido daria um número plausível e errado.
std::string describeAppliedVolume(const hog::HoggedDevice& hogged) {
    char text[64];
    float scalar = 0;
    double decibels = 0;
    if (hogged.readVolume(scalar, decibels)) {
        std::snprintf(text, sizeof(text), "%.0f%% (%.1f dB)", scalar * 100.0, decibels);
    } else {
        std::snprintf(text, sizeof(text), "desconhecido");
    }
    return text;
}

// Um pedido explícito é uma garantia: se não der para cumprir, é melhor não tocar do que
// tocar mais alto do que se pediu. Já o teto é uma rede de proteção — não havendo controle
// de volume, avisa e segue, que é o comportamento de sempre.
std::string applyVolume(hog::HoggedDevice& hogged, const hog::OutputDevice& device,
                        bool hasRequest, const hog::VolumeRequest& request, double ceiling) {
    if (hasRequest) {
        float scalar = 0;
        if (request.unit == hog::VolumeUnit::Percent) {
            scalar = static_cast<float>(request.value);
        } else if (!hogged.decibelsToScalar(request.value, scalar)) {
            return "este device não converte decibéis; use porcentagem";
        }

        const float before = device.volume;
        if (std::string error = hogged.setVolume(scalar); !error.empty()) return error;

        std::printf("volume   : %s", describeAppliedVolume(hogged).c_str());
        if (before >= 0) std::printf(" [era %.0f%%]", before * 100.0);
        std::printf("\n");
        return {};
    }

    if (device.volume < 0) {
        std::printf("volume   : device sem controle de volume; teto não aplicável\n");
        return {};
    }

    // O teto decide sobre o volume lido agora, não sobre o que havia antes de tomar o
    // device: entre uma coisa e outra o usuário pode ter mexido no volume, e a reconfiguração
    // do device também pode alterá-lo. Uma proteção que age sobre leitura velha não protege.
    float current = device.volume;
    double currentDecibels = 0;
    if (!hogged.readVolume(current, currentDecibels)) current = device.volume;

    const hog::CeilingDecision decision = hog::applyCeiling(current, ceiling);
    if (!decision.apply) {
        std::printf("volume   : %s (abaixo do teto de %.0f%%)\n",
                    describeAppliedVolume(hogged).c_str(), ceiling * 100.0);
        return {};
    }

    const float target = static_cast<float>(decision.scalar);
    if (std::string error = hogged.setVolume(target); !error.empty()) {
        std::printf("aviso    : volume em %.0f%% e não consegui baixá-lo (%s)\n", current * 100.0,
                    error.c_str());
        return {};
    }
    std::printf("volume   : %s [baixado do teto: estava em %.0f%%]\n",
                describeAppliedVolume(hogged).c_str(), current * 100.0);
    return {};
}

int usage() {
    std::fprintf(stderr,
                 "uso: hog-audio [--info] [--volume V] [--max-volume V] <arquivo>\n\n"
                 "  Reproduz o arquivo tomando o DAC em modo exclusivo, travado no sample\n"
                 "  rate e no bit depth do próprio arquivo. Ctrl+C interrompe.\n\n"
                 "  --info          mostra o que seria negociado, sem tocar no device\n"
                 "  --volume V      volume da reprodução: 35, 35%% ou -18dB\n"
                 "  --max-volume V  teto aplicado quando --volume é omitido (padrão 50%%)\n\n"
                 "  O volume é ajustado depois de tomar o device e antes de sair som, e é\n"
                 "  devolvido ao valor anterior ao terminar.\n");
    return 2;
}

}  // namespace

int main(int argc, char** argv) {
    // Sem isto, redirecionar a saída embaralha a ordem entre stdout e stderr.
    std::setvbuf(stdout, nullptr, _IOLBF, 0);

    // Antes de qualquer coisa que altere o device. Entre tomar o device e instalar os
    // handlers existiria uma janela em que um Ctrl+C mataria o processo pela disposição
    // padrão, sem rodar destrutor nenhum — e o sample rate ficaria trocado.
    // SIGHUP cobre o terminal sendo fechado; SIGQUIT, o Ctrl+\.
    for (int sig : {SIGINT, SIGTERM, SIGHUP, SIGQUIT}) std::signal(sig, onInterrupt);

    std::string path;
    std::string dumpPath;
    bool infoOnly = false;
    bool hasVolumeRequest = false;
    hog::VolumeRequest volumeRequest;
    double ceiling = kDefaultCeiling;

    for (int i = 1; i < argc; ++i) {
        const std::string arg = argv[i];
        if (arg == "--info") {
            infoOnly = true;
        } else if (arg == "-h" || arg == "--help") {
            return usage();
        } else if (arg == "--dump") {
            if (i + 1 >= argc) {
                std::fprintf(stderr, "erro: --dump exige um caminho de arquivo\n");
                return 2;
            }
            dumpPath = argv[++i];
        } else if (arg == "--volume" || arg == "--max-volume") {
            if (i + 1 >= argc) {
                std::fprintf(stderr, "erro: %s exige um valor\n", arg.c_str());
                return 2;
            }
            const hog::VolumeRequest parsed = hog::parseVolume(argv[++i]);
            if (!parsed.valid) {
                std::fprintf(stderr, "erro: %s\n", parsed.reason.c_str());
                return 2;
            }
            if (arg == "--volume") {
                volumeRequest = parsed;
                hasVolumeRequest = true;
            } else if (parsed.unit != hog::VolumeUnit::Percent) {
                std::fprintf(stderr, "erro: --max-volume aceita só porcentagem\n");
                return 2;
            } else {
                ceiling = parsed.value;
            }
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

    // O volume é resolvido aqui, com o device já nosso e antes de qualquer amostra sair: é o
    // único ponto em que dá para garantir que o fone não receba o volume anterior.
    if (std::string error = applyVolume(hogged, device, hasVolumeRequest, volumeRequest, ceiling);
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
        while (!interrupted()) {
            const std::uint32_t got = source.read(chunk.data(), framesPerChunk);
            if (got == 0) break;

            std::size_t written = 0;
            const std::size_t bytes = static_cast<std::size_t>(got) * client.mBytesPerFrame;
            while (written < bytes && !interrupted()) {
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
            g_interrupted.store(1, std::memory_order_relaxed);
            if (thread.joinable()) thread.join();
        }
    } producerGuard{producer};

    // Consome o ring exatamente como o IOProc faria, mas grava em disco. O pipeline é o
    // mesmo — decodificador, ring buffer, alinhamento de frame — porque um atalho que apenas
    // decodificasse pularia justamente as partes onde estiveram os bugs mais caros.
    if (!dumpPath.empty()) {
        constexpr std::size_t kDumpBlockFrames = 512;
        std::vector<std::uint8_t> block(kDumpBlockFrames * client.mBytesPerFrame);
        std::FILE* out = std::fopen(dumpPath.c_str(), "wb");
        if (out == nullptr) {
            std::fprintf(stderr, "erro: não consegui criar %s\n", dumpPath.c_str());
            return 1;
        }

        std::uint64_t framesWritten = 0;
        while (true) {
            const std::size_t take = hog::alignedReadSize(
                block.size(), playback.ring.availableToRead(), playback.bytesPerFrame);
            const std::size_t got = playback.ring.read(block.data(), take);
            if (got > 0) {
                std::fwrite(block.data(), 1, got, out);
                framesWritten += got / client.mBytesPerFrame;
                continue;
            }
            if (playback.producerDone.load(std::memory_order_acquire) || interrupted()) break;
            std::this_thread::sleep_for(std::chrono::milliseconds(2));
        }
        std::fclose(out);
        std::printf("dump     : %llu frames em %s\n",
                    static_cast<unsigned long long>(framesWritten), dumpPath.c_str());

        g_interrupted.store(1, std::memory_order_relaxed);
        producer.join();
        if (std::string error = hogged.finish(); !error.empty()) {
            std::fprintf(stderr, "aviso    : %s\n", error.c_str());
            return 1;
        }
        std::printf("fim      : device restaurado\n");
        return 0;
    }

    // Deixa o buffer encher antes de abrir o fluxo, para o começo da faixa não sair picotado.
    for (int i = 0; i < 200 && playback.ring.availableToRead() < ringBytes / 2 &&
                    !playback.producerDone.load(std::memory_order_acquire) && !interrupted();
         ++i) {
        std::this_thread::sleep_for(std::chrono::milliseconds(5));
    }

    if (std::string error = hogged.start(ioProc, &playback); !error.empty()) {
        std::fprintf(stderr, "erro: %s\n", error.c_str());
        return 1;  // producerGuard encerra a thread; o destrutor de hogged devolve o device
    }

    const double seconds = static_cast<double>(source.totalFrames()) / source.format().sampleRate;
    std::printf("tocando  : %.1f s — Ctrl+C interrompe\n", seconds);

    while (!interrupted() && !playback.finished.load(std::memory_order_acquire)) {
        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }

    const bool interruptedByUser = interrupted();

    hogged.stop();
    g_interrupted.store(1, std::memory_order_relaxed);  // desbloqueia a thread produtora, que espera espaço no ring
    producer.join();    // aqui é o caminho normal; o guard cobre os caminhos de exceção

    const std::uint64_t underruns = playback.underruns.load(std::memory_order_relaxed);
    if (underruns > 0) {
        std::printf("aviso    : %llu falhas de alimentação do buffer\n",
                    static_cast<unsigned long long>(underruns));
    }
    if (std::string error = hogged.finish(); !error.empty()) {
        std::fprintf(stderr,
                     "aviso    : %s\n"
                     "           o device pode ter ficado com outra configuração; tocar\n"
                     "           qualquer outro som ou abrir Configuração de Áudio e MIDI ajusta\n",
                     error.c_str());
        return 1;
    }

    std::printf("fim      : device restaurado\n");
    return interruptedByUser ? 130 : 0;  // 128 + SIGINT, como manda a convenção
}
