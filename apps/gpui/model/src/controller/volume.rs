//! Um escritor só, com o pedido mais recente sobrescrevendo o anterior. A ordem é garantida por
//! construção, e um arrasto inteiro custa poucas escritas em vez de uma por quadro: o hardware
//! precisa do valor em que o dedo parou, não dos intermediários.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use super::Wake;
use crate::port::Player;

#[derive(Default)]
struct Slot {
    desired: Option<(u64, f32)>,
    stop: bool,
}

#[derive(Default)]
struct Shared {
    slot: Mutex<Slot>,
    wake: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(|p| p.into_inner())
    }
}

pub(crate) enum VolumeResult {
    Written,
    Failed {
        message: String,
        device_volume: Option<f32>,
    },
    /// Parado no encerramento antes de chegar ao player.
    Discarded,
}

enum Next {
    Write(u64, f32),
    /// O pedido que ainda esperava no slot fica sem escrever, mas volta com o número dele para o
    /// `busy()` fechar a conta.
    Stop(Option<u64>),
}

struct Settled {
    through: u64,
    result: VolumeResult,
}

pub(crate) struct VolumeWorker {
    shared: Arc<Shared>,
    results: Receiver<Settled>,
    requested: u64,
    settled: u64,
}

impl VolumeWorker {
    pub fn spawn(player: Arc<dyn Player>, wake: Wake) -> Self {
        let shared = Arc::new(Shared::default());
        let (tx, results) = mpsc::channel();
        let worker = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("hog-volume".to_string())
            .spawn(move || {
                loop {
                    let (seq, value) = match next_request(&worker) {
                        Next::Write(seq, value) => (seq, value),
                        Next::Stop(discarded) => {
                            if let Some(seq) = discarded {
                                let settled = Settled {
                                    through: seq,
                                    result: VolumeResult::Discarded,
                                };
                                if tx.send(settled).is_ok() {
                                    wake();
                                }
                            }
                            return;
                        }
                    };
                    let settled = match player.set_volume(value) {
                        Ok(()) => Settled {
                            through: seq,
                            result: VolumeResult::Written,
                        },
                        Err(error) => {
                            // O resto do arrasto sai junto: insistir por cima de uma recusa só
                            // repetiria o erro.
                            let dropped =
                                worker.lock().desired.take().map_or(seq, |(later, _)| later);
                            Settled {
                                through: dropped.max(seq),
                                result: VolumeResult::Failed {
                                    message: error.to_string(),
                                    device_volume: player.snapshot().volume_scalar,
                                },
                            }
                        }
                    };
                    if tx.send(settled).is_err() {
                        return;
                    }
                    wake();
                }
            })
            .expect("não consegui criar a thread de volume");
        Self {
            shared,
            results,
            requested: 0,
            settled: 0,
        }
    }

    pub fn request(&mut self, value: f32) {
        self.requested += 1;
        self.shared.lock().desired = Some((self.requested, value));
        self.shared.wake.notify_one();
    }

    /// Há pedido que ainda não voltou: esperando, sendo escrito ou com a resposta no canal.
    pub fn busy(&self) -> bool {
        self.settled != self.requested
    }

    /// A recusa de um pedido que outro mais novo já superou não fala pelo slider: o volume do
    /// device que ela traz é de antes do pedido novo, e quem decide é a resposta dele.
    pub fn drain(&mut self) -> Vec<VolumeResult> {
        let mut results = Vec::new();
        while let Ok(settled) = self.results.try_recv() {
            self.settled = self.settled.max(settled.through);
            let superseded = settled.through < self.requested;
            results.push(match settled.result {
                VolumeResult::Failed { message, .. } if superseded => VolumeResult::Failed {
                    message,
                    device_volume: None,
                },
                result => result,
            });
        }
        results
    }

    pub fn stop(&self) {
        self.shared.lock().stop = true;
        self.shared.wake.notify_one();
    }
}

fn next_request(shared: &Shared) -> Next {
    let mut slot = shared.lock();
    loop {
        if slot.stop {
            return Next::Stop(slot.desired.take().map(|(seq, _)| seq));
        }
        if let Some((seq, value)) = slot.desired.take() {
            return Next::Write(seq, value);
        }
        slot = shared.wake.wait(slot).unwrap_or_else(|p| p.into_inner());
    }
}
