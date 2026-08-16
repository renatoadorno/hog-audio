#include "format_negotiation.hpp"

#include "check.hpp"

using namespace hog;

namespace {

PhysicalFormatDesc intFormat(int index, double rate, unsigned bits, unsigned channels = 2) {
    return {index, rate, SampleType::Integer, bits, channels};
}

PhysicalFormatDesc floatFormat(int index, double rate, unsigned channels = 2) {
    return {index, rate, SampleType::Float, 32, channels};
}

// Um DAC built-in típico do Apple Silicon: rates discretos, 24-bit inteiro em cada um.
DeviceCaps builtInLike() {
    return DeviceCaps{
        {{44100, 44100}, {48000, 48000}, {88200, 88200}, {96000, 96000}},
        {intFormat(0, 44100, 24), intFormat(1, 48000, 24), intFormat(2, 88200, 24),
         intFormat(3, 96000, 24)},
        2};
}

void rate_exatamente_suportado_e_aceito() {
    auto d = negotiate(FileFormat{96000, 24, 2}, builtInLike());

    CHECK(d.play);
    CHECK_EQ(d.sampleRate, 96000.0);
    CHECK_EQ(d.physicalFormatIndex, 3);
    CHECK(!d.duplicateMonoToStereo);
}

void rate_dentro_de_range_continuo_e_aceito() {
    DeviceCaps caps{{{32000, 192000}}, {intFormat(0, 176400, 24)}, 2};

    auto d = negotiate(FileFormat{176400, 24, 2}, caps);

    CHECK(d.play);
    CHECK_EQ(d.sampleRate, 176400.0);
}

void rate_nao_suportado_aborta() {
    auto d = negotiate(FileFormat{192000, 24, 2}, builtInLike());

    CHECK(!d.play);
    CHECK_EQ(d.physicalFormatIndex, -1);
    CHECK(d.reason.find("192000") != std::string::npos);
    CHECK(d.reason.find("96000") != std::string::npos);
}

void arquivo_16_bits_usa_formato_de_24_quando_e_o_unico() {
    auto d = negotiate(FileFormat{44100, 16, 2}, builtInLike());

    CHECK(d.play);
    CHECK_EQ(d.physicalFormatIndex, 0);
}

void escolhe_o_menor_bit_depth_que_comporta_o_arquivo() {
    DeviceCaps caps{{{44100, 44100}},
                    {intFormat(0, 44100, 32), intFormat(1, 44100, 16), intFormat(2, 44100, 24)},
                    2};

    auto d = negotiate(FileFormat{44100, 24, 2}, caps);

    CHECK(d.play);
    CHECK_EQ(d.physicalFormatIndex, 2);  // 24 comporta 24; 16 não; 32 é maior que o necessário
}

void nao_escolhe_bit_depth_menor_que_o_do_arquivo() {
    DeviceCaps caps{{{44100, 44100}}, {intFormat(0, 44100, 16)}, 2};

    auto d = negotiate(FileFormat{44100, 24, 2}, caps);

    CHECK(!d.play);
    CHECK(d.reason.find("24") != std::string::npos);
}

void float32_e_aceito_para_arquivo_de_ate_24_bits() {
    DeviceCaps caps{{{96000, 96000}}, {floatFormat(0, 96000)}, 2};

    auto d = negotiate(FileFormat{96000, 24, 2}, caps);

    CHECK(d.play);
    CHECK_EQ(d.physicalFormatIndex, 0);
}

void float32_nao_comporta_arquivo_de_32_bits_inteiros() {
    // float32 tem 24 bits de mantissa: um inteiro de 32 bits não sobrevive à ida e volta.
    DeviceCaps caps{{{96000, 96000}}, {floatFormat(0, 96000)}, 2};

    auto d = negotiate(FileFormat{96000, 32, 2}, caps);

    CHECK(!d.play);
    CHECK_EQ(d.physicalFormatIndex, -1);
}

void prefere_inteiro_a_float_no_mesmo_rate() {
    DeviceCaps caps{{{96000, 96000}}, {floatFormat(0, 96000), intFormat(1, 96000, 24)}, 2};

    auto d = negotiate(FileFormat{96000, 24, 2}, caps);

    CHECK(d.play);
    CHECK_EQ(d.physicalFormatIndex, 1);
}

void mono_e_duplicado_para_estereo() {
    auto d = negotiate(FileFormat{44100, 16, 1}, builtInLike());

    CHECK(d.play);
    CHECK(d.duplicateMonoToStereo);
}

void mais_canais_que_o_device_aborta() {
    auto d = negotiate(FileFormat{48000, 24, 6}, builtInLike());

    CHECK(!d.play);
    CHECK(d.reason.find("6") != std::string::npos);
}

void rate_suportado_mas_sem_formato_fisico_no_rate_aborta() {
    DeviceCaps caps{{{44100, 44100}, {96000, 96000}}, {intFormat(0, 44100, 24)}, 2};

    auto d = negotiate(FileFormat{96000, 24, 2}, caps);

    CHECK(!d.play);
    CHECK_EQ(d.physicalFormatIndex, -1);
}

void device_sem_formato_algum_aborta() {
    DeviceCaps caps{{{44100, 44100}}, {}, 2};

    auto d = negotiate(FileFormat{44100, 24, 2}, caps);

    CHECK(!d.play);
}

}  // namespace

int main() {
    rate_exatamente_suportado_e_aceito();
    rate_dentro_de_range_continuo_e_aceito();
    rate_nao_suportado_aborta();
    arquivo_16_bits_usa_formato_de_24_quando_e_o_unico();
    escolhe_o_menor_bit_depth_que_comporta_o_arquivo();
    nao_escolhe_bit_depth_menor_que_o_do_arquivo();
    float32_e_aceito_para_arquivo_de_ate_24_bits();
    float32_nao_comporta_arquivo_de_32_bits_inteiros();
    prefere_inteiro_a_float_no_mesmo_rate();
    mono_e_duplicado_para_estereo();
    mais_canais_que_o_device_aborta();
    rate_suportado_mas_sem_formato_fisico_no_rate_aborta();
    device_sem_formato_algum_aborta();
    return check::summary();
}
