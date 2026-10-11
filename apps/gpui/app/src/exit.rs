//! Sair sem deixar o device travado: log de transições, sinais do terminal e pânico. O caminho
//! normal (⌘Q, fechar a janela, sinal) sempre termina no `on_app_quit`, que chama o shutdown.

use std::ffi::c_int;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{App, Entity};
use hog_player_model::controller::LogSink;
use hog_player_model::port::Player;
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook::iterator::Signals;
use signal_hook::low_level::emulate_default_handler;

use crate::store::Store;

pub fn log_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Logs/hog-player-gpui.log"))
}

/// Stderr e um arquivo, no mesmo formato de linha do Swift. Rodando pelo .app o stderr se perde,
/// e é do arquivo que a medição 5 lê quando o medidor externo não serve.
pub fn transition_log(path: Option<PathBuf>) -> LogSink {
    let file = path.and_then(open_log_file).map(Mutex::new);
    Arc::new(move |line: &str| {
        to_stderr(format_args!("{line}"));
        let Some(file) = &file else {
            return;
        };
        // No pânico, o lock pode estar com a própria thread que caiu ou com uma travada, e
        // esperar por ele prenderia o hook: ocupado, a linha fica só no stderr.
        let mut file = if std::thread::panicking() {
            match file.try_lock() {
                Ok(file) => file,
                Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
                Err(TryLockError::WouldBlock) => return,
            }
        } else {
            file.lock().unwrap_or_else(|p| p.into_inner())
        };
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        let _ = writeln!(file, "{millis} {line}");
    })
}

/// O `eprintln!` entra em pânico quando a escrita falha (EPIPE ou EIO com o terminal fechado), e
/// o log roda na main e no panic hook, onde um pânico vira abort.
fn to_stderr(message: std::fmt::Arguments) {
    let _ = writeln!(std::io::stderr(), "{message}");
}

fn open_log_file(path: PathBuf) -> Option<File> {
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => Some(file),
        Err(error) => {
            to_stderr(format_args!(
                "o log de transições fica só no stderr: não abri {}: {error}",
                path.display()
            ));
            None
        }
    }
}

/// O ⌘Q, o botão vermelho da última janela e o sinal terminam aqui. Síncrono de propósito: o
/// future do `on_app_quit` tem teto de 200 ms, o corpo do closure não. Devolver o device não
/// pode ser cortado no meio.
pub fn shutdown_on_quit(store: &Entity<Store>, log: LogSink, cx: &mut App) {
    let store = store.clone();
    cx.on_app_quit(move |cx| {
        if let Err(error) = store.update(cx, |store, _| store.shutdown()) {
            log(&format!("falha ao encerrar o player: {error}"));
        }
        std::future::ready(())
    })
    .detach();
}

/// Ctrl+C, SIGTERM, SIGHUP e SIGQUIT viram um pedido de saída que o próximo tique atende, pelo
/// mesmo caminho do ⌘Q. O handler do signal-hook só marca: nada que não seja seguro em sinal.
pub fn watch_signals(quit: Arc<AtomicBool>) -> std::io::Result<()> {
    // A saída de emergência é para quando o shutdown trava: termina como o sistema terminaria,
    // e o device fica para o macOS soltar.
    watch_signals_with(
        quit,
        &[SIGINT, SIGTERM, SIGHUP, SIGQUIT],
        Instant::now,
        |signal| {
            if let Err(error) = emulate_default_handler(signal) {
                to_stderr(format_args!(
                    "sinal {signal} repetido sem ação padrão ({error}): abortando"
                ));
                std::process::abort();
            }
        },
    )
}

/// O `watch_signals` com os sinais, o relógio e a saída de emergência injetáveis: a de verdade
/// mata o processo, e o teste não pode morrer.
pub fn watch_signals_with(
    quit: Arc<AtomicBool>,
    signals: &[c_int],
    clock: impl Fn() -> Instant + Send + 'static,
    on_emergency: impl Fn(c_int) + Send + 'static,
) -> std::io::Result<()> {
    let mut signals = Signals::new(signals)?;
    std::thread::Builder::new()
        .name("hog-signals".to_string())
        .spawn(move || {
            // O `Signals` vive com a thread. Solto, ele desregistra os handlers e o processo passa
            // a ignorar esses sinais (aviso do `unregister` no signal-hook-registry): o segundo
            // Ctrl+C não faria nada.
            let mut first_at: Option<Instant> = None;
            for signal in signals.forever() {
                let now = clock();
                quit.store(true, Ordering::Release);
                let Some(first) = first_at else {
                    first_at = Some(now);
                    continue;
                };
                if is_emergency(signal, now.saturating_duration_since(first)) {
                    on_emergency(signal);
                }
            }
        })?;
    Ok(())
}

/// Se um sinal que chega com a saída já pedida há `since_first` é a saída de emergência. O
/// SIGHUP nunca é: fechar a aba do terminal entrega dois em milissegundos (o do zsh e o do
/// kernel ao grupo do primeiro plano). Os outros só contam depois de 1 s, para que o segundo
/// sinal não chegue antes do tique de 100 ms e pule o `on_app_quit`, deixando a taxa trocada.
pub fn is_emergency(signal: c_int, since_first: Duration) -> bool {
    const GRACE: Duration = Duration::from_secs(1);
    signal != SIGHUP && since_first >= GRACE
}

/// Um pânico na thread principal aborta o processo, porque os callbacks do AppKit são
/// `extern "C"`. Antes disso, o hook tenta devolver o device, com teto: se o engine estiver
/// travado no próprio pânico, desiste e deixa o macOS soltar o hog. O rate fica trocado (a
/// armadilha conhecida).
pub fn install_panic_hook(player: Arc<dyn Player>, log: LogSink) {
    install_panic_hook_with(player, log, is_main_thread);
}

/// Só o pânico na thread principal derruba o app. Os outros podem ser recuperados por um
/// `catch_unwind` (as cargas da fila, o callback do HAL no core), e um `shutdown` ali deixaria o
/// player recusando tudo com a janela aberta.
fn is_main_thread() -> bool {
    std::thread::current().name() == Some("main")
}

/// O `install_panic_hook` com o predicado de pânico fatal injetável: o libtest nunca roda teste
/// na thread principal.
pub fn install_panic_hook_with(
    player: Arc<dyn Player>,
    log: LogSink,
    is_fatal: impl Fn() -> bool + Send + Sync + 'static,
) {
    const RESTORE_LIMIT: Duration = Duration::from_millis(500);
    // Uma tentativa só: um pânico dentro da restauração não dispara outra em cadeia.
    let attempted = AtomicBool::new(false);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        if !is_fatal() || attempted.swap(true, Ordering::AcqRel) {
            return;
        }
        let player = Arc::clone(&player);
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("hog-panic-restore".to_string())
            .spawn(move || {
                let _ = tx.send(player.shutdown());
            });
        if spawned.is_err() {
            log("pânico: não consegui criar a thread para devolver o device");
            return;
        }
        match rx.recv_timeout(RESTORE_LIMIT) {
            Ok(Ok(())) => log("pânico: device devolvido"),
            Ok(Err(error)) => log(&format!("pânico: falha ao devolver o device: {error}")),
            Err(RecvTimeoutError::Timeout) => log(&format!(
                "pânico: o engine não respondeu em {} ms",
                RESTORE_LIMIT.as_millis()
            )),
            Err(RecvTimeoutError::Disconnected) => {
                log("pânico: a restauração falhou ou entrou em pânico")
            }
        }
    }));
}
