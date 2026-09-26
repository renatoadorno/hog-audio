//! Os campos que a interface consulta continuamente. Ficam fora do mutex do engine de
//! propósito: a interface lê dez vezes por segundo, e um comando lento — a aquisição do hog
//! leva centenas de milissegundos — travaria a janela se disputasse o mesmo lock.
//!
//! O IOProc escreve `frames_rendered` e `underruns` daqui. Por isso tudo é atômico: são as
//! únicas operações que um callback de tempo real pode fazer com segurança.

use std::sync::atomic::{AtomicI64, AtomicU8, AtomicU32, AtomicU64, Ordering};

use crate::transitions::PlayerState;

pub struct SharedStatus {
    state: AtomicU8,
    frames_rendered: AtomicU64,
    underruns: AtomicU64,
    clipped: AtomicU64,
    total_frames: AtomicI64,
    sample_rate_bits: AtomicU64,
    volume_bits: AtomicU32,
    current_index: AtomicU64,
    queue_len: AtomicU32,
    queue_version: AtomicU64,
}

fn state_to_u8(state: PlayerState) -> u8 {
    match state {
        PlayerState::Idle => 0,
        PlayerState::Loaded => 1,
        PlayerState::Playing => 2,
        PlayerState::Paused => 3,
        PlayerState::Finished => 4,
        PlayerState::Failed => 5,
    }
}

fn state_from_u8(value: u8) -> PlayerState {
    match value {
        1 => PlayerState::Loaded,
        2 => PlayerState::Playing,
        3 => PlayerState::Paused,
        4 => PlayerState::Finished,
        5 => PlayerState::Failed,
        _ => PlayerState::Idle,
    }
}

impl SharedStatus {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(state_to_u8(PlayerState::Idle)),
            frames_rendered: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            clipped: AtomicU64::new(0),
            total_frames: AtomicI64::new(0),
            sample_rate_bits: AtomicU64::new(0f64.to_bits()),
            volume_bits: AtomicU32::new(0f32.to_bits()),
            current_index: AtomicU64::new(u64::MAX),
            queue_len: AtomicU32::new(0),
            queue_version: AtomicU64::new(0),
        }
    }

    pub fn set_state(&self, state: PlayerState) {
        self.state.store(state_to_u8(state), Ordering::Release);
    }

    pub fn state(&self) -> PlayerState {
        state_from_u8(self.state.load(Ordering::Acquire))
    }

    pub fn set_track(&self, total_frames: i64, sample_rate: f64) {
        // A interface lê estes campos concorrentemente, sem lock e sem esperar este método
        // terminar. Zerar o progresso antes de publicar total/taxa é o que garante isso: na
        // ordem inversa, uma leitura intercalada veria o total da faixa nova com o decorrido
        // da faixa anterior e mostraria a faixa nova já no fim.
        self.reset_progress();
        self.sample_rate_bits
            .store(sample_rate.to_bits(), Ordering::Release);
        self.total_frames.store(total_frames, Ordering::Release);
    }

    pub fn reset_progress(&self) {
        self.frames_rendered.store(0, Ordering::Release);
        self.underruns.store(0, Ordering::Release);
        self.clipped.store(0, Ordering::Release);
    }

    /// Chamado pelo IOProc. `Relaxed` basta: ninguém sincroniza dados com este contador.
    pub fn add_frames(&self, frames: u64) {
        self.frames_rendered.fetch_add(frames, Ordering::Relaxed);
    }

    pub fn elapsed_seconds(&self) -> f64 {
        // A taxa é lida uma única vez porque um set_track() pode correr entre dois loads
        // independentes; ler via total_seconds() aqui misturaria o decorrido de uma taxa
        // com o total de outra.
        let rate = f64::from_bits(self.sample_rate_bits.load(Ordering::Acquire));
        if rate <= 0.0 {
            return 0.0;
        }
        let elapsed = self.frames_rendered.load(Ordering::Relaxed) as f64 / rate;
        let total = self.total_frames.load(Ordering::Acquire) as f64 / rate;
        if total > 0.0 && elapsed > total {
            total
        } else {
            elapsed
        }
    }

    pub fn total_seconds(&self) -> f64 {
        let rate = f64::from_bits(self.sample_rate_bits.load(Ordering::Acquire));
        if rate <= 0.0 {
            return 0.0;
        }
        self.total_frames.load(Ordering::Acquire) as f64 / rate
    }

    pub fn add_underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }

    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    /// Amostras que a afinação levou além do fundo de escala. Diferente de underrun: não é
    /// falha de alimentação, é sinal distorcido — e sem contador ninguém descobre, porque
    /// clipping não interrompe nada, só soa pior.
    pub fn add_clipped(&self, count: u64) {
        self.clipped.fetch_add(count, Ordering::Relaxed);
    }

    pub fn clipped(&self) -> u64 {
        self.clipped.load(Ordering::Relaxed)
    }

    pub fn set_volume(&self, scalar: f32) {
        self.volume_bits.store(scalar.to_bits(), Ordering::Release);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Acquire))
    }

    pub fn set_queue_state(&self, current: Option<usize>, len: usize) {
        self.current_index.store(
            current.map_or(u64::MAX, |index| index as u64),
            Ordering::Release,
        );
        self.queue_len.store(len as u32, Ordering::Release);
    }

    pub fn current_index(&self) -> Option<u32> {
        let index = self.current_index.load(Ordering::Acquire);
        (index != u64::MAX).then_some(index as u32)
    }

    pub fn queue_len(&self) -> u32 {
        self.queue_len.load(Ordering::Acquire)
    }

    pub fn bump_queue_version(&self) {
        self.queue_version.fetch_add(1, Ordering::AcqRel);
    }

    pub fn queue_version(&self) -> u64 {
        self.queue_version.load(Ordering::Acquire)
    }
}

impl Default for SharedStatus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_estado_sobrevive_a_ida_e_volta_para_u8() {
        // Um mapeamento errado aqui é silencioso: a interface mostraria "pausado" com o
        // player tocando, sem nenhum erro em lugar nenhum.
        for estado in [
            PlayerState::Idle,
            PlayerState::Loaded,
            PlayerState::Playing,
            PlayerState::Paused,
            PlayerState::Finished,
            PlayerState::Failed,
        ] {
            let status = SharedStatus::new();
            status.set_state(estado);
            assert_eq!(status.state(), estado);
        }
    }

    #[test]
    fn comeca_em_idle_e_zerado() {
        let status = SharedStatus::new();
        assert_eq!(status.state(), PlayerState::Idle);
        assert_eq!(status.elapsed_seconds(), 0.0);
        assert_eq!(status.underruns(), 0);
    }

    #[test]
    fn frames_viram_segundos_pela_taxa_de_amostragem() {
        let status = SharedStatus::new();
        status.set_track(96_000 * 10, 96_000.0);
        status.add_frames(48_000);
        assert!((status.elapsed_seconds() - 0.5).abs() < 1e-9);
        assert!((status.total_seconds() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn o_decorrido_nunca_passa_da_duracao_total() {
        // O último bloco do IOProc conta o silêncio de preenchimento; sem o teto a interface
        // mostraria a faixa passando do fim.
        let status = SharedStatus::new();
        status.set_track(1_000, 1_000.0);
        status.add_frames(5_000);
        assert!((status.elapsed_seconds() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sem_faixa_carregada_o_decorrido_e_zero() {
        let status = SharedStatus::new();
        status.add_frames(500);
        assert_eq!(status.elapsed_seconds(), 0.0);
    }

    #[test]
    fn reset_zera_o_progresso_mas_preserva_a_faixa() {
        let status = SharedStatus::new();
        status.set_track(96_000, 96_000.0);
        status.add_frames(48_000);
        status.add_underrun();
        status.reset_progress();
        assert_eq!(status.elapsed_seconds(), 0.0);
        assert_eq!(status.underruns(), 0);
        assert!((status.total_seconds() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn o_volume_sobrevive_a_ida_e_volta() {
        let status = SharedStatus::new();
        status.set_volume(0.375);
        assert!((status.volume() - 0.375).abs() < 1e-6);
    }

    #[test]
    fn estado_barato_da_fila_sobrevive_a_ida_e_volta() {
        let status = SharedStatus::new();
        assert_eq!(status.current_index(), None);
        assert_eq!(status.queue_len(), 0);
        assert_eq!(status.queue_version(), 0);

        status.set_queue_state(Some(7), 12);
        status.bump_queue_version();
        assert_eq!(status.current_index(), Some(7));
        assert_eq!(status.queue_len(), 12);
        assert_eq!(status.queue_version(), 1);
    }
}
