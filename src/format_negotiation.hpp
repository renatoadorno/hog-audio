#pragma once

// Núcleo puro da decisão de formato. Nenhuma dependência de Core Audio, para que a regra
// que determina se a reprodução é bit-perfect possa ser testada sem tocar em hardware.

#include <string>
#include <vector>

namespace hog {

enum class SampleType { Integer, Float };

// Um formato físico oferecido pelo device. `index` aponta para a posição do
// AudioStreamBasicDescription correspondente na lista que a camada HAL enumerou: o núcleo
// decide qual usar, a camada HAL sabe como aplicá-lo.
struct PhysicalFormatDesc {
    int index;
    double sampleRate;
    SampleType sampleType;
    unsigned bitsPerChannel;
    unsigned channels;
};

// Devices podem publicar rates discretos (minimum == maximum) ou faixas contínuas.
struct SampleRateRange {
    double minimum;
    double maximum;
};

struct FileFormat {
    double sampleRate;
    unsigned bitDepth;
    unsigned channels;
};

struct DeviceCaps {
    std::vector<SampleRateRange> rates;
    std::vector<PhysicalFormatDesc> physicalFormats;
    unsigned outputChannels;
};

struct Decision {
    bool play = false;
    double sampleRate = 0;
    int physicalFormatIndex = -1;
    bool duplicateMonoToStereo = false;
    std::string reason;
};

// Decide se o arquivo pode ser reproduzido sem resample e sem perda de bits, e com qual
// formato físico. Nunca escolhe um formato que exija conversão destrutiva: quando não há
// caminho íntegro, devolve play == false com o motivo preenchido.
Decision negotiate(const FileFormat& file, const DeviceCaps& caps);

}  // namespace hog
