#include "volume.hpp"

#include <cctype>

namespace hog {
namespace {

std::string trimmed(const std::string& text) {
    std::size_t first = 0;
    while (first < text.size() && std::isspace(static_cast<unsigned char>(text[first]))) ++first;
    std::size_t last = text.size();
    while (last > first && std::isspace(static_cast<unsigned char>(text[last - 1]))) --last;
    return text.substr(first, last - first);
}

bool endsWithDecibels(const std::string& text, std::string& numberPart) {
    if (text.size() < 3) return false;
    const std::string suffix = text.substr(text.size() - 2);
    const bool isDb = (suffix[0] == 'd' || suffix[0] == 'D') && (suffix[1] == 'b' || suffix[1] == 'B');
    if (!isDb) return false;
    numberPart = trimmed(text.substr(0, text.size() - 2));
    return !numberPart.empty();
}

// Conversão própria em vez de std::stod: queremos recusar sobras como "35x", que stod
// aceitaria silenciosamente devolvendo 35.
bool parseNumber(const std::string& text, double& out) {
    if (text.empty()) return false;

    std::size_t i = 0;
    double sign = 1;
    if (text[i] == '+' || text[i] == '-') {
        if (text[i] == '-') sign = -1;
        ++i;
    }
    if (i >= text.size()) return false;

    double whole = 0;
    bool sawDigit = false;
    for (; i < text.size() && std::isdigit(static_cast<unsigned char>(text[i])); ++i) {
        whole = whole * 10 + (text[i] - '0');
        sawDigit = true;
    }
    if (i < text.size() && text[i] == '.') {
        ++i;
        double scale = 0.1;
        for (; i < text.size() && std::isdigit(static_cast<unsigned char>(text[i])); ++i) {
            whole += (text[i] - '0') * scale;
            scale *= 0.1;
            sawDigit = true;
        }
    }
    if (!sawDigit || i != text.size()) return false;

    out = sign * whole;
    return true;
}

}  // namespace

VolumeRequest parseVolume(const std::string& text) {
    const std::string input = trimmed(text);
    if (input.empty()) return {false, VolumeUnit::Percent, 0, "volume vazio"};

    std::string numberPart;
    if (endsWithDecibels(input, numberPart)) {
        double db = 0;
        if (!parseNumber(numberPart, db)) {
            return {false, VolumeUnit::Decibels, 0, "não entendi os decibéis em \"" + text + "\""};
        }
        if (db > 0) {
            return {false, VolumeUnit::Decibels, 0,
                    "o amplificador atenua a partir de 0 dB; não há ganho acima disso"};
        }
        return {true, VolumeUnit::Decibels, db, {}};
    }

    std::string percentPart = input;
    if (percentPart.back() == '%') percentPart = trimmed(percentPart.substr(0, percentPart.size() - 1));

    double percent = 0;
    if (!parseNumber(percentPart, percent)) {
        return {false, VolumeUnit::Percent, 0,
                "não entendi \"" + text + "\"; use algo como 35, 35% ou -18dB"};
    }
    if (percent < 0 || percent > 100) {
        return {false, VolumeUnit::Percent, 0, "porcentagem fora de 0 a 100"};
    }
    return {true, VolumeUnit::Percent, percent / 100.0, {}};
}

CeilingDecision applyCeiling(double currentScalar, double ceilingScalar) {
    if (currentScalar <= ceilingScalar) return {};
    return {true, ceilingScalar};
}

}  // namespace hog
