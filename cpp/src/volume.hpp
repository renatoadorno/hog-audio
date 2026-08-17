#pragma once

// Interpretação do volume pedido e regra do teto de segurança. Puro, sem Core Audio: a
// conversão entre porcentagem e decibéis depende da curva do amplificador e fica na camada
// que fala com o hardware, mas decidir o que foi pedido não precisa de device nenhum.

#include <string>

namespace hog {

enum class VolumeUnit { Percent, Decibels };

struct VolumeRequest {
    bool valid = false;
    VolumeUnit unit = VolumeUnit::Percent;
    double value = 0;  // 0..1 quando Percent; decibéis (≤ 0) quando Decibels
    std::string reason;
};

// Aceita "35", "35%", "12.5", "-18dB", "-18 db". Recusa faixas impossíveis em vez de as
// truncar em silêncio: um volume interpretado errado chega ao fone como volume errado.
VolumeRequest parseVolume(const std::string& text);

struct CeilingDecision {
    bool apply = false;
    double scalar = 0;
};

// Sem pedido explícito, o volume só é tocado quando passa do teto — para que esquecer a
// flag não signifique receber o volume cheio no fone.
CeilingDecision applyCeiling(double currentScalar, double ceilingScalar);

}  // namespace hog
