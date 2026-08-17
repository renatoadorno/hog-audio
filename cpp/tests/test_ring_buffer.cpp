#include "ring_buffer.hpp"

#include <cstdint>
#include <thread>
#include <vector>

#include "check.hpp"

using namespace hog;

namespace {

std::vector<uint8_t> sequence(size_t n, uint8_t start = 0) {
    std::vector<uint8_t> v(n);
    for (size_t i = 0; i < n; ++i) v[i] = static_cast<uint8_t>(start + i);
    return v;
}

void buffer_novo_esta_vazio() {
    RingBuffer rb(1024);

    CHECK_EQ(rb.availableToRead(), size_t{0});
    CHECK_EQ(rb.availableToWrite(), rb.capacity());
}

void capacidade_e_arredondada_para_potencia_de_dois() {
    CHECK_EQ(RingBuffer(1000).capacity(), size_t{1024});
    CHECK_EQ(RingBuffer(1024).capacity(), size_t{1024});
    CHECK_EQ(RingBuffer(1025).capacity(), size_t{2048});
}

void o_que_entra_e_o_que_sai() {
    RingBuffer rb(64);
    auto in = sequence(10);
    std::vector<uint8_t> out(10, 0xFF);

    CHECK_EQ(rb.write(in.data(), in.size()), size_t{10});
    CHECK_EQ(rb.availableToRead(), size_t{10});
    CHECK_EQ(rb.read(out.data(), out.size()), size_t{10});

    CHECK(out == in);
    CHECK_EQ(rb.availableToRead(), size_t{0});
}

void escrita_nao_ultrapassa_a_capacidade() {
    RingBuffer rb(16);
    auto in = sequence(100);

    CHECK_EQ(rb.write(in.data(), in.size()), size_t{16});
    CHECK_EQ(rb.availableToWrite(), size_t{0});
}

void leitura_devolve_so_o_que_existe() {
    RingBuffer rb(64);
    auto in = sequence(5);
    std::vector<uint8_t> out(50, 0xFF);

    rb.write(in.data(), in.size());

    CHECK_EQ(rb.read(out.data(), out.size()), size_t{5});
    CHECK_EQ(out[0], uint8_t{0});
    CHECK_EQ(out[4], uint8_t{4});
    CHECK_EQ(out[5], uint8_t{0xFF});  // não escreveu além do que tinha
}

void wrap_around_preserva_os_dados() {
    RingBuffer rb(16);
    std::vector<uint8_t> out(16, 0);

    // Consome quase todo o buffer para empurrar os índices para perto do fim.
    auto primeiro = sequence(12, 100);
    rb.write(primeiro.data(), primeiro.size());
    rb.read(out.data(), 12);

    // Esta escrita cruza a fronteira física do buffer.
    auto segundo = sequence(10, 200);
    CHECK_EQ(rb.write(segundo.data(), segundo.size()), size_t{10});

    std::vector<uint8_t> lido(10, 0);
    CHECK_EQ(rb.read(lido.data(), lido.size()), size_t{10});
    CHECK(lido == segundo);
}

// Uma escrita que cruza a borda, lida em fragmentos de tamanhos diferentes. Se a escrita
// ignorar a borda e transbordar, a leitura fragmentada usa offsets que não coincidem com os
// do transbordo e o erro aparece — ao contrário de uma leitura única, que repetiria o mesmo
// engano e devolveria o dado "certo" lido de fora do buffer.
void wrap_around_sobrevive_a_leitura_fragmentada() {
    RingBuffer rb(16);
    std::vector<uint8_t> descarte(12, 0);

    auto primeiro = sequence(12, 100);
    rb.write(primeiro.data(), primeiro.size());
    rb.read(descarte.data(), 12);

    auto segundo = sequence(10, 200);  // 4 bytes cabem até a borda, 6 voltam ao início
    CHECK_EQ(rb.write(segundo.data(), segundo.size()), size_t{10});

    std::vector<uint8_t> inicio(4, 0), resto(6, 0);
    CHECK_EQ(rb.read(inicio.data(), inicio.size()), size_t{4});
    CHECK_EQ(rb.read(resto.data(), resto.size()), size_t{6});

    CHECK(inicio == std::vector<uint8_t>(segundo.begin(), segundo.begin() + 4));
    CHECK(resto == std::vector<uint8_t>(segundo.begin() + 4, segundo.end()));
}

// O espelho do caso acima: escrita fragmentada cruzando a borda, leitura única.
void wrap_around_sobrevive_a_escrita_fragmentada() {
    RingBuffer rb(16);
    std::vector<uint8_t> descarte(12, 0);

    auto primeiro = sequence(12, 100);
    rb.write(primeiro.data(), primeiro.size());
    rb.read(descarte.data(), 12);

    auto a = sequence(3, 200);
    auto b = sequence(7, 210);
    rb.write(a.data(), a.size());
    rb.write(b.data(), b.size());

    std::vector<uint8_t> lido(10, 0);
    CHECK_EQ(rb.read(lido.data(), lido.size()), size_t{10});

    std::vector<uint8_t> esperado = a;
    esperado.insert(esperado.end(), b.begin(), b.end());
    CHECK(lido == esperado);
}

void ler_tudo_devolve_o_buffer_ao_estado_vazio() {
    RingBuffer rb(32);
    auto in = sequence(32);
    std::vector<uint8_t> out(32, 0);

    rb.write(in.data(), in.size());
    CHECK_EQ(rb.availableToWrite(), size_t{0});
    rb.read(out.data(), out.size());

    CHECK_EQ(rb.availableToRead(), size_t{0});
    CHECK_EQ(rb.availableToWrite(), size_t{32});
    CHECK(out == in);
}

// Produtor e consumidor em threads separadas: a sequência lida tem de sair intacta e na
// ordem, que é a única garantia da qual o IOProc depende.
void produtor_e_consumidor_concorrentes_preservam_a_sequencia() {
    constexpr size_t kTotal = 1 << 20;
    RingBuffer rb(4096);

    std::thread produtor([&] {
        size_t escrito = 0;
        while (escrito < kTotal) {
            uint8_t byte = static_cast<uint8_t>(escrito);
            escrito += rb.write(&byte, 1);
        }
    });

    size_t lido = 0;
    bool intacto = true;
    while (lido < kTotal) {
        uint8_t byte = 0;
        if (rb.read(&byte, 1) == 1) {
            if (byte != static_cast<uint8_t>(lido)) intacto = false;
            ++lido;
        }
    }
    produtor.join();

    CHECK(intacto);
    CHECK_EQ(lido, kTotal);
}

// --- alinhamento de frame na leitura -------------------------------------------------
// O produtor escreve bytes crus e pode parar no meio de um frame quando o buffer enche.
// Se o callback consumir esse resto parcial, o índice de leitura sai de fase com a
// fronteira de frame e NÃO volta: da próxima leitura em diante, bytes do canal esquerdo
// são lidos como direito. O sintoma é ruído, e é permanente.

void leitura_alinhada_descarta_o_frame_parcial() {
    // 20 bytes disponíveis com frames de 6 bytes: só 18 podem ser consumidos.
    CHECK_EQ(alignedReadSize(24, 20, 6), size_t{18});
}

void leitura_alinhada_respeita_o_que_foi_pedido() {
    CHECK_EQ(alignedReadSize(24, 100, 6), size_t{24});
    CHECK_EQ(alignedReadSize(24, 24, 6), size_t{24});
}

void sem_um_frame_completo_nao_se_le_nada() {
    CHECK_EQ(alignedReadSize(24, 4, 6), size_t{0});
    CHECK_EQ(alignedReadSize(24, 0, 6), size_t{0});
}

void frames_potencia_de_dois_tambem_sao_alinhados() {
    CHECK_EQ(alignedReadSize(24, 20, 8), size_t{16});
}

void frame_degenerado_nao_le_nada() {
    CHECK_EQ(alignedReadSize(24, 20, 0), size_t{0});
}

// O caso que o desalinhamento provoca de verdade: depois de um underrun com resto parcial,
// a leitura seguinte tem de começar exatamente onde um frame começa.
void fase_do_frame_sobrevive_a_um_underrun() {
    constexpr unsigned kFrame = 6;  // 24 bits, 2 canais — não é potência de dois
    RingBuffer rb(64);

    auto dados = sequence(20);  // 3 frames completos + 2 bytes soltos
    rb.write(dados.data(), dados.size());

    std::vector<uint8_t> saida(24, 0xFF);
    const size_t primeira = alignedReadSize(24, rb.availableToRead(), kFrame);
    CHECK_EQ(rb.read(saida.data(), primeira), size_t{18});

    // Chega o resto do fluxo; a próxima leitura tem de retomar no byte 18, início de frame.
    auto resto = sequence(10, 20);
    rb.write(resto.data(), resto.size());

    std::vector<uint8_t> segunda(12, 0);
    const size_t n = alignedReadSize(12, rb.availableToRead(), kFrame);
    rb.read(segunda.data(), n);

    CHECK_EQ(segunda[0], uint8_t{18});  // e não 20, que seria a fase perdida
    CHECK_EQ(segunda[1], uint8_t{19});
}

}  // namespace

int main() {
    leitura_alinhada_descarta_o_frame_parcial();
    leitura_alinhada_respeita_o_que_foi_pedido();
    sem_um_frame_completo_nao_se_le_nada();
    frames_potencia_de_dois_tambem_sao_alinhados();
    frame_degenerado_nao_le_nada();
    fase_do_frame_sobrevive_a_um_underrun();
    buffer_novo_esta_vazio();
    capacidade_e_arredondada_para_potencia_de_dois();
    o_que_entra_e_o_que_sai();
    escrita_nao_ultrapassa_a_capacidade();
    leitura_devolve_so_o_que_existe();
    wrap_around_preserva_os_dados();
    wrap_around_sobrevive_a_leitura_fragmentada();
    wrap_around_sobrevive_a_escrita_fragmentada();
    ler_tudo_devolve_o_buffer_ao_estado_vazio();
    produtor_e_consumidor_concorrentes_preservam_a_sequencia();
    return check::summary();
}
