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

// Interfaces profissionais publicam o formato físico com faixa contínua em vez de uma taxa
// fixa. Recusar só porque a taxa do arquivo não é o extremo da faixa seria um falso negativo.
void formato_fisico_de_faixa_continua_aceita_taxa_interna() {
    PhysicalFormatDesc contínuo{0, 44100, SampleType::Integer, 24, 2};
    contínuo.sampleRateMinimum = 44100;
    contínuo.sampleRateMaximum = 192000;
    DeviceCaps caps{{{44100, 192000}}, {contínuo}, 2};

    auto d = negotiate(FileFormat{96000, 24, 2}, caps);

    CHECK(d.play);
    CHECK_EQ(d.sampleRate, 96000.0);
    CHECK_EQ(d.physicalFormatIndex, 0);
}

void formato_fisico_de_faixa_continua_recusa_taxa_fora_da_faixa() {
    PhysicalFormatDesc contínuo{0, 44100, SampleType::Integer, 24, 2};
    contínuo.sampleRateMinimum = 44100;
    contínuo.sampleRateMaximum = 96000;
    DeviceCaps caps{{{44100, 192000}}, {contínuo}, 2};

    auto d = negotiate(FileFormat{192000, 24, 2}, caps);

    CHECK(!d.play);
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

// --- validação do formato de entrega -------------------------------------------------
// Um descasamento entre o que o decodificador produz e o que o device espera não degrada
// o som: vira ruído branco em volume total, capaz de danificar fone e audição. Por isso a
// consistência é verificada antes de qualquer amostra chegar ao DAC.

void formatos_consistentes_sao_aceitos() {
    CHECK(validateInterleavedFormat(32, 8, 2).ok);   // float32 estéreo
    CHECK(validateInterleavedFormat(16, 4, 2).ok);   // int16 estéreo
    CHECK(validateInterleavedFormat(24, 6, 2).ok);   // int24 packed
    CHECK(validateInterleavedFormat(24, 8, 2).ok);   // int24 em container de 32 bits
    CHECK(validateInterleavedFormat(32, 4, 1).ok);   // float32 mono
}

void container_menor_que_a_amostra_e_recusado() {
    // 32 bits por canal não cabem em 2 bytes por canal.
    auto r = validateInterleavedFormat(32, 4, 2);

    CHECK(!r.ok);
    CHECK(!r.reason.empty());
}

void bytes_por_frame_indivisivel_pelos_canais_e_recusado() {
    CHECK(!validateInterleavedFormat(16, 5, 2).ok);
}

void profundidade_fora_do_byte_e_recusada() {
    CHECK(!validateInterleavedFormat(20, 8, 2).ok);
}

void formato_degenerado_e_recusado() {
    CHECK(!validateInterleavedFormat(16, 4, 0).ok);
    CHECK(!validateInterleavedFormat(0, 4, 2).ok);
    CHECK(!validateInterleavedFormat(16, 0, 2).ok);
}

}  // namespace

int main() {
    formatos_consistentes_sao_aceitos();
    container_menor_que_a_amostra_e_recusado();
    bytes_por_frame_indivisivel_pelos_canais_e_recusado();
    profundidade_fora_do_byte_e_recusada();
    formato_degenerado_e_recusado();
    rate_exatamente_suportado_e_aceito();
    rate_dentro_de_range_continuo_e_aceito();
    formato_fisico_de_faixa_continua_aceita_taxa_interna();
    formato_fisico_de_faixa_continua_recusa_taxa_fora_da_faixa();
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
