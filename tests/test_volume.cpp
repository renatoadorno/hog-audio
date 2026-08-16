#include "volume.hpp"

#include "check.hpp"

using namespace hog;

namespace {

bool near(double a, double b) {
    return (a - b) < 1e-9 && (b - a) < 1e-9;
}

// --- interpretação do argumento -------------------------------------------------------

void porcentagem_simples() {
    auto v = parseVolume("35");

    CHECK(v.valid);
    CHECK(v.unit == VolumeUnit::Percent);
    CHECK(near(v.value, 0.35));
}

void porcentagem_com_sinal_de_percentual() {
    auto v = parseVolume("35%");

    CHECK(v.valid);
    CHECK(v.unit == VolumeUnit::Percent);
    CHECK(near(v.value, 0.35));
}

void extremos_de_porcentagem() {
    CHECK(near(parseVolume("0").value, 0.0));
    CHECK(near(parseVolume("100").value, 1.0));
    CHECK(parseVolume("0").valid);
    CHECK(parseVolume("100").valid);
}

void porcentagem_fracionaria() {
    auto v = parseVolume("12.5");

    CHECK(v.valid);
    CHECK(near(v.value, 0.125));
}

void decibeis_em_qualquer_caixa() {
    for (const char* texto : {"-18dB", "-18db", "-18DB", "-18 dB"}) {
        auto v = parseVolume(texto);
        CHECK(v.valid);
        CHECK(v.unit == VolumeUnit::Decibels);
        CHECK(near(v.value, -18.0));
    }
}

void zero_decibeis_e_valido() {
    auto v = parseVolume("0dB");

    CHECK(v.valid);
    CHECK(v.unit == VolumeUnit::Decibels);
    CHECK(near(v.value, 0.0));
}

void decibeis_positivos_sao_recusados() {
    // O amp atenua a partir de 0 dB; pedir ganho acima disso não existe no hardware.
    auto v = parseVolume("6dB");

    CHECK(!v.valid);
    CHECK(!v.reason.empty());
}

void porcentagem_fora_da_faixa_e_recusada() {
    CHECK(!parseVolume("101").valid);
    CHECK(!parseVolume("-5").valid);
}

void texto_sem_sentido_e_recusado() {
    CHECK(!parseVolume("").valid);
    CHECK(!parseVolume("abc").valid);
    CHECK(!parseVolume("35x").valid);
    CHECK(!parseVolume("dB").valid);
    CHECK(!parseVolume("--").valid);
}

// --- teto de segurança ----------------------------------------------------------------

void volume_acima_do_teto_e_baixado() {
    auto d = applyCeiling(1.0, 0.5);

    CHECK(d.apply);
    CHECK(near(d.scalar, 0.5));
}

void volume_abaixo_do_teto_fica_como_esta() {
    auto d = applyCeiling(0.3, 0.5);

    CHECK(!d.apply);
}

void volume_exatamente_no_teto_nao_e_mexido() {
    auto d = applyCeiling(0.5, 0.5);

    CHECK(!d.apply);
}

}  // namespace

int main() {
    porcentagem_simples();
    porcentagem_com_sinal_de_percentual();
    extremos_de_porcentagem();
    porcentagem_fracionaria();
    decibeis_em_qualquer_caixa();
    zero_decibeis_e_valido();
    decibeis_positivos_sao_recusados();
    porcentagem_fora_da_faixa_e_recusada();
    texto_sem_sentido_e_recusado();
    volume_acima_do_teto_e_baixado();
    volume_abaixo_do_teto_fica_como_esta();
    volume_exatamente_no_teto_nao_e_mexido();
    return check::summary();
}
