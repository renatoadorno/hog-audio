//! Quanto cada etapa de uma troca de faixa levou, para medir em vez de estimar. Só observa:
//! nenhuma etapa muda de ordem nem de conteúdo por causa disto, e nada daqui roda no IOProc.
//!
//! O mutex é folha — quem o segura nunca pede o lock do engine —, então ele pode ser gravado
//! com o engine travado e lido pela interface sem esperar um comando lento terminar.

use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhaseTimings {
    pub open_ms: f64,
    pub release_ms: f64,
    pub reopen_ms: f64,
    pub acquire_ms: f64,
    pub volume_ms: f64,
    pub prefill_ms: f64,
    pub start_ms: f64,
}

#[derive(Clone, Copy, Debug)]
pub enum Phase {
    Open,
    Release,
    Reopen,
    Acquire,
    Volume,
    Prefill,
    Start,
}

#[derive(Default)]
pub struct TimingLog(Mutex<PhaseTimings>);

impl TimingLog {
    pub fn reset(&self) {
        *self.lock() = PhaseTimings::default();
    }

    /// Soma em vez de sobrescrever: um comando pode passar pela mesma etapa mais de uma vez —
    /// o `advance` que pula faixas ruins abre um arquivo por tentativa, e o `start_common`
    /// chama `stop_and_release` de novo depois da carga.
    pub fn record(&self, phase: Phase, since: Instant) {
        let elapsed_ms = since.elapsed().as_secs_f64() * 1000.0;
        let mut timings = self.lock();
        let slot = match phase {
            Phase::Open => &mut timings.open_ms,
            Phase::Release => &mut timings.release_ms,
            Phase::Reopen => &mut timings.reopen_ms,
            Phase::Acquire => &mut timings.acquire_ms,
            Phase::Volume => &mut timings.volume_ms,
            Phase::Prefill => &mut timings.prefill_ms,
            Phase::Start => &mut timings.start_ms,
        };
        *slot += elapsed_ms;
    }

    pub fn snapshot(&self) -> PhaseTimings {
        *self.lock()
    }

    fn lock(&self) -> MutexGuard<'_, PhaseTimings> {
        self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mesma_etapa_soma_e_o_reset_zera() {
        let log = TimingLog::default();
        let started = Instant::now() - std::time::Duration::from_millis(10);
        log.record(Phase::Open, started);
        log.record(Phase::Open, started);
        let timings = log.snapshot();
        assert!(timings.open_ms >= 20.0);
        assert_eq!(timings.release_ms, 0.0);

        log.reset();
        assert_eq!(log.snapshot(), PhaseTimings::default());
    }
}
