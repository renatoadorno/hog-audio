#include "format_negotiation.hpp"

#include <cmath>

namespace hog {
namespace {

// Rates são valores inteiros na prática (44100, 96000...), separados por milhares.
bool sameRate(double a, double b) {
    return std::fabs(a - b) < 0.5;
}

std::string rateText(double rate) {
    return std::to_string(static_cast<long long>(std::llround(rate)));
}

std::string supportedRatesText(const std::vector<SampleRateRange>& rates) {
    std::string out;
    for (const auto& r : rates) {
        if (!out.empty()) out += ", ";
        out += sameRate(r.minimum, r.maximum)
                   ? rateText(r.minimum)
                   : rateText(r.minimum) + "-" + rateText(r.maximum);
    }
    return out.empty() ? "nenhum" : out;
}

bool rateIsSupported(double rate, const std::vector<SampleRateRange>& rates) {
    for (const auto& r : rates) {
        if (rate >= r.minimum - 0.5 && rate <= r.maximum + 0.5) return true;
    }
    return false;
}

// float32 tem 24 bits de mantissa: inteiros de até 24 bits sobrevivem à ida e volta sem
// perda. Acima disso a conversão deixaria de ser exata e o formato é recusado.
constexpr unsigned kFloatExactBits = 24;

bool preservesEveryBit(const PhysicalFormatDesc& fmt, unsigned fileBitDepth) {
    if (fmt.sampleType == SampleType::Float) return fileBitDepth <= kFloatExactBits;
    return fmt.bitsPerChannel >= fileBitDepth;
}

// Entre os formatos íntegros: inteiro ganha de float, e o menor bit depth suficiente ganha
// dos maiores — trocar bits de largura não perde informação, mas o menor é o mais direto.
bool isBetter(const PhysicalFormatDesc& candidate, const PhysicalFormatDesc& current) {
    if (candidate.sampleType != current.sampleType) {
        return candidate.sampleType == SampleType::Integer;
    }
    return candidate.bitsPerChannel < current.bitsPerChannel;
}

}  // namespace

FormatCheck validateInterleavedFormat(unsigned bitsPerChannel, unsigned bytesPerFrame,
                                      unsigned channels) {
    if (channels == 0) return {false, "formato sem canais"};
    if (bitsPerChannel == 0) return {false, "formato sem profundidade de bits"};
    if (bytesPerFrame == 0) return {false, "formato sem tamanho de frame"};
    if (bitsPerChannel % 8 != 0) {
        return {false, "profundidade de " + std::to_string(bitsPerChannel) +
                           " bits não é múltipla de 8; o layout no buffer seria ambíguo"};
    }
    if (bytesPerFrame % channels != 0) {
        return {false, "frame de " + std::to_string(bytesPerFrame) + " bytes não divide entre " +
                           std::to_string(channels) + " canais"};
    }

    // O container por canal pode ser maior que a amostra (int24 dentro de 32 bits), nunca menor.
    const unsigned bytesPerChannel = bytesPerFrame / channels;
    if (bytesPerChannel * 8 < bitsPerChannel) {
        return {false, "container de " + std::to_string(bytesPerChannel) +
                           " bytes por canal não comporta amostras de " +
                           std::to_string(bitsPerChannel) + " bits"};
    }
    return {true, {}};
}

Decision negotiate(const FileFormat& file, const DeviceCaps& caps) {
    Decision d;

    if (file.channels == 0) {
        d.reason = "arquivo sem canais de áudio";
        return d;
    }
    if (file.channels > caps.outputChannels) {
        d.reason = "arquivo tem " + std::to_string(file.channels) + " canais; o device oferece " +
                   std::to_string(caps.outputChannels);
        return d;
    }

    if (!rateIsSupported(file.sampleRate, caps.rates)) {
        d.reason = "arquivo em " + rateText(file.sampleRate) +
                   " Hz; o device suporta: " + supportedRatesText(caps.rates) +
                   ". Reproduzir exigiria resample.";
        return d;
    }

    const PhysicalFormatDesc* best = nullptr;
    for (const auto& fmt : caps.physicalFormats) {
        if (!sameRate(fmt.sampleRate, file.sampleRate)) continue;
        if (!preservesEveryBit(fmt, file.bitDepth)) continue;
        if (best == nullptr || isBetter(fmt, *best)) best = &fmt;
    }

    if (best == nullptr) {
        d.reason = "nenhum formato físico do device em " + rateText(file.sampleRate) +
                   " Hz comporta " + std::to_string(file.bitDepth) + " bits sem perda";
        return d;
    }

    d.play = true;
    d.sampleRate = file.sampleRate;
    d.physicalFormatIndex = best->index;
    d.duplicateMonoToStereo = file.channels == 1 && caps.outputChannels >= 2;
    d.reason = rateText(file.sampleRate) + " Hz / " + std::to_string(best->bitsPerChannel) +
               " bits " + (best->sampleType == SampleType::Float ? "float" : "inteiro");
    return d;
}

}  // namespace hog
