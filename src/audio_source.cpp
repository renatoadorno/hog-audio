#include "audio_source.hpp"

#include <cctype>

namespace hog {
namespace {

// Formatos comprimidos (FLAC, ALAC) deixam mBitsPerChannel zerado e codificam a
// profundidade da fonte nos format flags, com a mesma convenção do Apple Lossless.
unsigned bitDepthFromSourceFlags(UInt32 flags) {
    switch (flags) {
        case kAppleLosslessFormatFlag_16BitSourceData: return 16;
        case kAppleLosslessFormatFlag_20BitSourceData: return 20;
        case kAppleLosslessFormatFlag_24BitSourceData: return 24;
        case kAppleLosslessFormatFlag_32BitSourceData: return 32;
        default: return 0;
    }
}

std::string fourCC(UInt32 value) {
    const char chars[4] = {static_cast<char>((value >> 24) & 0xFF),
                           static_cast<char>((value >> 16) & 0xFF),
                           static_cast<char>((value >> 8) & 0xFF),
                           static_cast<char>(value & 0xFF)};
    for (char c : chars) {
        if (!std::isprint(static_cast<unsigned char>(c))) return std::to_string(value);
    }
    return std::string(chars, 4);
}

}  // namespace

std::string osStatusText(OSStatus status) {
    return fourCC(static_cast<UInt32>(status)) + " (" + std::to_string(status) + ")";
}

AudioSource::~AudioSource() {
    if (file_ != nullptr) ExtAudioFileDispose(file_);
}

std::string AudioSource::open(const std::string& path) {
    CFURLRef url = CFURLCreateFromFileSystemRepresentation(
        nullptr, reinterpret_cast<const UInt8*>(path.c_str()),
        static_cast<CFIndex>(path.size()), false);
    if (url == nullptr) return "caminho inválido: " + path;

    OSStatus status = ExtAudioFileOpenURL(url, &file_);
    CFRelease(url);
    if (status != noErr) return "não consegui abrir o arquivo: " + osStatusText(status);

    AudioStreamBasicDescription asbd{};
    UInt32 size = sizeof(asbd);
    status = ExtAudioFileGetProperty(file_, kExtAudioFileProperty_FileDataFormat, &size, &asbd);
    if (status != noErr) return "não consegui ler o formato do arquivo: " + osStatusText(status);

    format_.sampleRate = asbd.mSampleRate;
    format_.channels = asbd.mChannelsPerFrame;
    format_.bitDepth = asbd.mBitsPerChannel != 0 ? asbd.mBitsPerChannel
                                                 : bitDepthFromSourceFlags(asbd.mFormatFlags);
    codecName_ = fourCC(asbd.mFormatID);

    if (format_.bitDepth == 0) {
        return "não consegui determinar a profundidade de bits de " + codecName_ +
               "; sem isso não dá para garantir reprodução sem perda";
    }

    size = sizeof(totalFrames_);
    status = ExtAudioFileGetProperty(file_, kExtAudioFileProperty_FileLengthFrames, &size,
                                     &totalFrames_);
    if (status != noErr) return "não consegui ler a duração: " + osStatusText(status);

    return {};
}

std::string AudioSource::setClientFormat(const AudioStreamBasicDescription& asbd) {
    OSStatus status = ExtAudioFileSetProperty(file_, kExtAudioFileProperty_ClientDataFormat,
                                              sizeof(asbd), &asbd);
    if (status != noErr) {
        return "o decodificador recusou o formato de entrega: " + osStatusText(status);
    }
    clientBytesPerFrame_ = asbd.mBytesPerFrame;
    return {};
}

std::uint32_t AudioSource::read(void* dst, std::uint32_t frames) {
    AudioBufferList list{};
    list.mNumberBuffers = 1;
    list.mBuffers[0].mNumberChannels = 0;  // irrelevante para dados intercalados
    list.mBuffers[0].mDataByteSize = frames * clientBytesPerFrame_;
    list.mBuffers[0].mData = dst;

    UInt32 framesRead = frames;
    if (ExtAudioFileRead(file_, &framesRead, &list) != noErr) return 0;
    return framesRead;
}

}  // namespace hog
