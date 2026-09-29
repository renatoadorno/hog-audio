//! A thread de comandos: executa as chamadas bloqueantes ao core na ordem de chegada. Todas
//! podem soltar e tomar o mesmo device, então a ordem tem de ser preservada fora da main.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hog_audio::api::{PlayerError, TrackFormat};
use hog_audio::transitions::PlayerState;

use super::Wake;
use crate::audio_files::audio_files;
use crate::port::{Format, Player, Timings};

pub(crate) const NO_AUDIO_FOUND: &str = "nenhum arquivo de áudio encontrado";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JobKind {
    Toggle,
    Navigate,
    Select,
    Remove,
    Clear,
    Add,
    Advance,
    Recover,
}

pub(crate) enum Job {
    Toggle,
    Navigate,
    Select(u64),
    Remove(u64),
    Clear,
    Add(Vec<PathBuf>),
    Advance,
    Recover,
}

impl Job {
    fn kind(&self) -> JobKind {
        match self {
            Job::Toggle => JobKind::Toggle,
            Job::Navigate => JobKind::Navigate,
            Job::Select(_) => JobKind::Select,
            Job::Remove(_) => JobKind::Remove,
            Job::Clear => JobKind::Clear,
            Job::Add(_) => JobKind::Add,
            Job::Advance => JobKind::Advance,
            Job::Recover => JobKind::Recover,
        }
    }
}

pub(crate) struct Envelope {
    pub job: Job,
    pub queued_at: Instant,
}

/// O que a main e a thread de comandos dividem. O resto é só da main.
pub(crate) struct Shared {
    pub shutting_down: AtomicBool,
    /// Os passos de Next/Previous que ainda esperam a vez. Cliques somam aqui; a thread zera ao
    /// começar, e daí em diante um clique novo abre outra navegação.
    pending_navigation: Mutex<Option<i32>>,
    /// O último `problem_seq` que a main já mostrou; a thread o lê no começo de cada comando.
    pub shown_problem_seq: AtomicU64,
}

impl Shared {
    pub fn new(problem_seq: u64) -> Self {
        Self {
            shutting_down: AtomicBool::new(false),
            pending_navigation: Mutex::new(None),
            shown_problem_seq: AtomicU64::new(problem_seq),
        }
    }

    pub fn navigation(&self) -> MutexGuard<'_, Option<i32>> {
        self.pending_navigation
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }
}

pub(crate) struct Report {
    pub command: String,
    pub queued_at: Instant,
    pub started_at: Instant,
    pub finished_at: Instant,
    pub timings: Timings,
    pub problem_seq_at_start: u64,
}

impl Report {
    /// O mesmo formato de linha do Swift (`logTransition`), para os dois logs se compararem.
    pub fn line(&self) -> String {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let t = &self.timings;
        format!(
            "{}: espera {:.0} ms, comando {:.0} ms (open {:.1}, release {:.1}, reopen {:.1}, \
             acquire {:.1}, volume {:.1}, prefill {:.1}, start {:.1})",
            self.command,
            ms(self.started_at - self.queued_at),
            ms(self.finished_at - self.started_at),
            t.open_ms,
            t.release_ms,
            t.reopen_ms,
            t.acquire_ms,
            t.volume_ms,
            t.prefill_ms,
            t.start_ms
        )
    }
}

pub(crate) enum Effect {
    Nothing,
    Toggled {
        result: Result<(), String>,
        device_volume: Option<f32>,
    },
    Loaded(Result<Format, String>),
    Advanced(Result<Option<Format>, String>),
    Removed(Result<Option<Format>, String>),
    Cleared(Result<(), String>),
    AddFailed(String),
}

pub(crate) struct Outcome {
    pub kind: JobKind,
    pub effect: Effect,
    pub report: Option<Report>,
}

impl Outcome {
    fn nothing(kind: JobKind) -> Self {
        Self {
            kind,
            effect: Effect::Nothing,
            report: None,
        }
    }
}

pub(crate) fn spawn(
    player: Arc<dyn Player>,
    shared: Arc<Shared>,
    wake: Wake,
) -> (Sender<Envelope>, Receiver<Outcome>) {
    let (jobs, job_rx) = mpsc::channel::<Envelope>();
    let (outcome_tx, outcomes) = mpsc::channel();
    std::thread::Builder::new()
        .name("hog-commands".to_string())
        .spawn(move || {
            while let Ok(envelope) = job_rx.recv() {
                if outcome_tx
                    .send(run(player.as_ref(), &shared, envelope))
                    .is_err()
                {
                    return;
                }
                wake();
            }
        })
        .expect("não consegui criar a thread de comandos");
    (jobs, outcomes)
}

fn run(player: &dyn Player, shared: &Shared, envelope: Envelope) -> Outcome {
    let kind = envelope.job.kind();
    // A navegação sai da espera antes do guard: daqui em diante, um clique novo abre outra.
    let steps = if kind == JobKind::Navigate {
        shared.navigation().take().unwrap_or(0)
    } else {
        0
    };
    if shared.shutting_down.load(Ordering::Acquire) {
        return Outcome::nothing(kind);
    }
    let queued_at = envelope.queued_at;
    match envelope.job {
        Job::Toggle => toggle(player, shared, queued_at),
        Job::Navigate if steps == 0 => Outcome::nothing(kind),
        Job::Navigate => timed(
            kind,
            player,
            shared,
            queued_at,
            format!("navegação {steps}"),
            || Effect::Loaded(loaded(player.navigate(steps))),
        ),
        Job::Select(id) => timed(kind, player, shared, queued_at, "seleção", || {
            Effect::Loaded(loaded(player.select_track_id(id)))
        }),
        // Removida a atual, o engine já carregou quem assumiu e devolve o formato dela.
        Job::Remove(id) => timed(kind, player, shared, queued_at, "remoção", || {
            Effect::Removed(maybe_loaded(player.remove_track_id(id)))
        }),
        Job::Clear => Outcome {
            kind,
            effect: Effect::Cleared(player.clear_queue().map_err(|e| e.to_string())),
            report: None,
        },
        Job::Add(paths) => add(player, shared, queued_at, &paths),
        Job::Advance => timed(kind, player, shared, queued_at, "auto-avanço", || {
            Effect::Advanced(maybe_loaded(player.advance()))
        }),
        Job::Recover => {
            // A única falha possível é o player já estar encerrando.
            let _ = player.recover();
            Outcome::nothing(kind)
        }
    }
}

/// Decide entre tocar e pausar pelo estado lido na hora de executar, não na hora do clique:
/// dois toggles rápidos tocam e depois pausam.
fn toggle(player: &dyn Player, shared: &Shared, queued_at: Instant) -> Outcome {
    let state = player.snapshot().state;
    let pause = state == PlayerState::Playing;
    let play = matches!(
        state,
        PlayerState::Loaded | PlayerState::Paused | PlayerState::Finished
    );
    if !pause && !play {
        return Outcome::nothing(JobKind::Toggle);
    }
    let command = if pause { "pause" } else { "play" };
    timed(JobKind::Toggle, player, shared, queued_at, command, || {
        let result = if pause { player.pause() } else { player.play() };
        // Só o play publica o volume real (o teto de segurança pode ter agido): a tela relê.
        Effect::Toggled {
            result: result.map_err(|e| e.to_string()),
            device_volume: player.snapshot().volume_scalar,
        }
    })
}

fn add(player: &dyn Player, shared: &Shared, queued_at: Instant, paths: &[PathBuf]) -> Outcome {
    let was_empty = player.snapshot().queue_len == 0;
    let files = audio_files(paths);
    if files.is_empty() {
        return Outcome {
            kind: JobKind::Add,
            effect: Effect::AddFailed(NO_AUDIO_FOUND.to_string()),
            report: None,
        };
    }
    player.append_tracks(
        files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
    );
    if !was_empty {
        return Outcome::nothing(JobKind::Add);
    }
    // `append_tracks` não propaga a falha da carga inicial; selecionar a posição 0 torna o erro
    // visível.
    timed(
        JobKind::Add,
        player,
        shared,
        queued_at,
        "carga inicial",
        || Effect::Loaded(loaded(player.select_track(0))),
    )
}

fn timed(
    kind: JobKind,
    player: &dyn Player,
    shared: &Shared,
    queued_at: Instant,
    command: impl Into<String>,
    work: impl FnOnce() -> Effect,
) -> Outcome {
    let problem_seq_at_start = shared.shown_problem_seq.load(Ordering::Acquire);
    let started_at = Instant::now();
    let effect = work();
    let finished_at = Instant::now();
    Outcome {
        kind,
        effect,
        report: Some(Report {
            command: command.into(),
            queued_at,
            started_at,
            finished_at,
            timings: player.last_timings().into(),
            problem_seq_at_start,
        }),
    }
}

fn loaded(result: Result<TrackFormat, PlayerError>) -> Result<Format, String> {
    result
        .map(|format| Format::from(&format))
        .map_err(|e| e.to_string())
}

fn maybe_loaded(
    result: Result<Option<TrackFormat>, PlayerError>,
) -> Result<Option<Format>, String> {
    result
        .map(|format| format.as_ref().map(Format::from))
        .map_err(|e| e.to_string())
}
