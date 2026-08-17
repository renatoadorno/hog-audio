//! Fila de bytes de produtor único / consumidor único, sem locks, e a regra de alinhamento
//! de frame que a acompanha.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Quantos bytes podem ser consumidos sem cortar um frame ao meio.
///
/// O produtor escreve bytes crus e pode parar no meio de um frame quando o buffer enche. Se o
/// consumidor levar esse resto parcial, o índice de leitura sai de fase com a fronteira de
/// frame e não volta mais: amostras passam a ser lidas trocadas entre canais, o que soa como
/// ruído. Deixar o resto no buffer para a próxima rodada preserva a fase.
pub fn aligned_read_size(requested: usize, available: usize, bytes_per_frame: u32) -> usize {
    if bytes_per_frame == 0 {
        return 0;
    }
    let usable = requested.min(available);
    usable - (usable % bytes_per_frame as usize)
}

/// Fila SPSC sem locks. O consumidor é o IOProc, que roda em thread de tempo real: ele não
/// pode alocar, travar nem bloquear.
///
/// Os índices são monotônicos e só recebem a máscara na hora de indexar. Isso distingue
/// "cheio" de "vazio" sem sacrificar um slot: a diferença write - read é o conteúdo real.
///
/// `write` e `read` tomam `&self` para que produtor e consumidor vivam em threads distintas.
/// A segurança vem do contrato: **um único produtor e um único consumidor**. Chamar `write`
/// de duas threads, ou `read` de duas threads, é uso incorreto.
pub struct RingBuffer {
    data: UnsafeCell<Box<[u8]>>,
    capacity: usize,
    write_index: AtomicUsize,
    read_index: AtomicUsize,
}

unsafe impl Sync for RingBuffer {}
unsafe impl Send for RingBuffer {}

impl RingBuffer {
    pub fn new(capacity_bytes: usize) -> Self {
        let capacity = round_up_to_power_of_two(capacity_bytes);
        Self {
            data: UnsafeCell::new(vec![0u8; capacity].into_boxed_slice()),
            capacity,
            write_index: AtomicUsize::new(0),
            read_index: AtomicUsize::new(0),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn available_to_read(&self) -> usize {
        self.write_index.load(Ordering::Acquire) - self.read_index.load(Ordering::Acquire)
    }

    pub fn available_to_write(&self) -> usize {
        self.capacity - self.available_to_read()
    }

    /// Escreve até `src.len()`; devolve quanto coube. Só o produtor chama.
    pub fn write(&self, src: &[u8]) -> usize {
        let w = self.write_index.load(Ordering::Relaxed);
        let r = self.read_index.load(Ordering::Acquire);
        let n = src.len().min(self.capacity - (w - r));
        if n == 0 {
            return 0;
        }

        let offset = w & (self.capacity - 1);
        let until_edge = n.min(self.capacity - offset);
        // Seguro sob o contrato SPSC: esta região está além de write_index, então o
        // consumidor não a alcança até o store abaixo publicá-la.
        let data = unsafe { &mut *self.data.get() };
        data[offset..offset + until_edge].copy_from_slice(&src[..until_edge]);
        if n > until_edge {
            data[..n - until_edge].copy_from_slice(&src[until_edge..n]);
        }

        self.write_index.store(w + n, Ordering::Release);
        n
    }

    /// Lê até `dst.len()`; devolve quanto havia. Só o consumidor chama.
    pub fn read(&self, dst: &mut [u8]) -> usize {
        let r = self.read_index.load(Ordering::Relaxed);
        let w = self.write_index.load(Ordering::Acquire);
        let n = dst.len().min(w - r);
        if n == 0 {
            return 0;
        }

        let offset = r & (self.capacity - 1);
        let until_edge = n.min(self.capacity - offset);
        // Seguro sob o contrato SPSC: esta região já foi publicada pelo produtor e ele não
        // a sobrescreve enquanto read_index não avançar.
        let data = unsafe { &*self.data.get() };
        dst[..until_edge].copy_from_slice(&data[offset..offset + until_edge]);
        if n > until_edge {
            dst[until_edge..n].copy_from_slice(&data[..n - until_edge]);
        }

        self.read_index.store(r + n, Ordering::Release);
        n
    }
}

fn round_up_to_power_of_two(n: usize) -> usize {
    let mut p = 1usize;
    while p < n {
        p <<= 1;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn sequence(n: usize, start: u8) -> Vec<u8> {
        (0..n).map(|i| start.wrapping_add(i as u8)).collect()
    }

    #[test]
    fn buffer_novo_esta_vazio() {
        let rb = RingBuffer::new(1024);

        assert_eq!(rb.available_to_read(), 0);
        assert_eq!(rb.available_to_write(), rb.capacity());
    }

    #[test]
    fn capacidade_e_arredondada_para_potencia_de_dois() {
        assert_eq!(RingBuffer::new(1000).capacity(), 1024);
        assert_eq!(RingBuffer::new(1024).capacity(), 1024);
        assert_eq!(RingBuffer::new(1025).capacity(), 2048);
    }

    #[test]
    fn o_que_entra_e_o_que_sai() {
        let rb = RingBuffer::new(64);
        let entrada = sequence(10, 0);
        let mut saida = vec![0xFFu8; 10];

        assert_eq!(rb.write(&entrada), 10);
        assert_eq!(rb.available_to_read(), 10);
        assert_eq!(rb.read(&mut saida), 10);

        assert_eq!(saida, entrada);
        assert_eq!(rb.available_to_read(), 0);
    }

    #[test]
    fn escrita_nao_ultrapassa_a_capacidade() {
        let rb = RingBuffer::new(16);

        assert_eq!(rb.write(&sequence(100, 0)), 16);
        assert_eq!(rb.available_to_write(), 0);
    }

    #[test]
    fn leitura_devolve_so_o_que_existe() {
        let rb = RingBuffer::new(64);
        let mut saida = vec![0xFFu8; 50];

        rb.write(&sequence(5, 0));

        assert_eq!(rb.read(&mut saida), 5);
        assert_eq!(saida[0], 0);
        assert_eq!(saida[4], 4);
        assert_eq!(saida[5], 0xFF); // não escreveu além do que tinha
    }

    #[test]
    fn wrap_around_preserva_os_dados() {
        let rb = RingBuffer::new(16);
        let mut descarte = vec![0u8; 16];

        // Consome quase todo o buffer para empurrar os índices para perto do fim.
        rb.write(&sequence(12, 100));
        rb.read(&mut descarte[..12]);

        // Esta escrita cruza a fronteira física do buffer.
        let segundo = sequence(10, 200);
        assert_eq!(rb.write(&segundo), 10);

        let mut lido = vec![0u8; 10];
        assert_eq!(rb.read(&mut lido), 10);
        assert_eq!(lido, segundo);
    }

    // Uma escrita que cruza a borda, lida em fragmentos de tamanhos diferentes. Se a escrita
    // ignorar a borda e transbordar, a leitura fragmentada usa offsets que não coincidem com
    // os do transbordo e o erro aparece — ao contrário de uma leitura única, que repetiria o
    // mesmo engano e devolveria o dado "certo" lido de fora do buffer.
    #[test]
    fn wrap_around_sobrevive_a_leitura_fragmentada() {
        let rb = RingBuffer::new(16);
        let mut descarte = vec![0u8; 12];

        rb.write(&sequence(12, 100));
        rb.read(&mut descarte);

        let segundo = sequence(10, 200); // 4 bytes até a borda, 6 voltam ao início
        assert_eq!(rb.write(&segundo), 10);

        let mut inicio = vec![0u8; 4];
        let mut resto = vec![0u8; 6];
        assert_eq!(rb.read(&mut inicio), 4);
        assert_eq!(rb.read(&mut resto), 6);

        assert_eq!(inicio, segundo[..4]);
        assert_eq!(resto, segundo[4..]);
    }

    // O espelho do caso acima: escrita fragmentada cruzando a borda, leitura única.
    #[test]
    fn wrap_around_sobrevive_a_escrita_fragmentada() {
        let rb = RingBuffer::new(16);
        let mut descarte = vec![0u8; 12];

        rb.write(&sequence(12, 100));
        rb.read(&mut descarte);

        let a = sequence(3, 200);
        let b = sequence(7, 210);
        rb.write(&a);
        rb.write(&b);

        let mut lido = vec![0u8; 10];
        assert_eq!(rb.read(&mut lido), 10);

        let mut esperado = a.clone();
        esperado.extend_from_slice(&b);
        assert_eq!(lido, esperado);
    }

    #[test]
    fn ler_tudo_devolve_o_buffer_ao_estado_vazio() {
        let rb = RingBuffer::new(32);
        let entrada = sequence(32, 0);
        let mut saida = vec![0u8; 32];

        rb.write(&entrada);
        assert_eq!(rb.available_to_write(), 0);
        rb.read(&mut saida);

        assert_eq!(rb.available_to_read(), 0);
        assert_eq!(rb.available_to_write(), 32);
        assert_eq!(saida, entrada);
    }

    // Produtor e consumidor em threads separadas: a sequência lida tem de sair intacta e na
    // ordem, que é a única garantia da qual o IOProc depende.
    #[test]
    fn produtor_e_consumidor_concorrentes_preservam_a_sequencia() {
        const TOTAL: usize = 1 << 20;
        let rb = Arc::new(RingBuffer::new(4096));

        let produtor = {
            let rb = Arc::clone(&rb);
            std::thread::spawn(move || {
                let mut escrito = 0usize;
                while escrito < TOTAL {
                    let byte = [escrito as u8];
                    escrito += rb.write(&byte);
                }
            })
        };

        let mut lido = 0usize;
        let mut intacto = true;
        while lido < TOTAL {
            let mut byte = [0u8; 1];
            if rb.read(&mut byte) == 1 {
                if byte[0] != lido as u8 {
                    intacto = false;
                }
                lido += 1;
            }
        }
        produtor.join().unwrap();

        assert!(intacto);
        assert_eq!(lido, TOTAL);
    }

    // --- alinhamento de frame na leitura ---------------------------------------------

    #[test]
    fn leitura_alinhada_descarta_o_frame_parcial() {
        // 20 bytes disponíveis com frames de 6 bytes: só 18 podem ser consumidos.
        assert_eq!(aligned_read_size(24, 20, 6), 18);
    }

    #[test]
    fn leitura_alinhada_respeita_o_que_foi_pedido() {
        assert_eq!(aligned_read_size(24, 100, 6), 24);
        assert_eq!(aligned_read_size(24, 24, 6), 24);
    }

    #[test]
    fn sem_um_frame_completo_nao_se_le_nada() {
        assert_eq!(aligned_read_size(24, 4, 6), 0);
        assert_eq!(aligned_read_size(24, 0, 6), 0);
    }

    #[test]
    fn frames_potencia_de_dois_tambem_sao_alinhados() {
        assert_eq!(aligned_read_size(24, 20, 8), 16);
    }

    #[test]
    fn frame_degenerado_nao_le_nada() {
        assert_eq!(aligned_read_size(24, 20, 0), 0);
    }

    // O caso que o desalinhamento provoca de verdade: depois de um underrun com resto
    // parcial, a leitura seguinte tem de começar exatamente onde um frame começa.
    #[test]
    fn fase_do_frame_sobrevive_a_um_underrun() {
        const FRAME: u32 = 6; // 24 bits, 2 canais — não é potência de dois
        let rb = RingBuffer::new(64);

        rb.write(&sequence(20, 0)); // 3 frames completos + 2 bytes soltos

        let mut saida = vec![0xFFu8; 24];
        let primeira = aligned_read_size(24, rb.available_to_read(), FRAME);
        assert_eq!(rb.read(&mut saida[..primeira]), 18);

        // Chega o resto do fluxo; a próxima leitura tem de retomar no byte 18.
        rb.write(&sequence(10, 20));

        let mut segunda = vec![0u8; 12];
        let n = aligned_read_size(12, rb.available_to_read(), FRAME);
        rb.read(&mut segunda[..n]);

        assert_eq!(segunda[0], 18); // e não 20, que seria a fase perdida
        assert_eq!(segunda[1], 19);
    }
}
