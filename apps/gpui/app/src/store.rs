//! A ponte entre o controlador e as views. Guarda o `Controller`, faz o poll, e notifica só o
//! sinal de cada parte do modelo que mudou: é o equivalente do `@Observable` do SwiftUI.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use gpui::{App, AppContext, Context, Entity, Task};
use hog_player_model::controller::{Changes, Config, Controller, Model, Wake};
use hog_player_model::port::Player;

/// Uma entidade vazia por parte do modelo. A parte que muda notifica a sua, e só a view que a
/// observa redesenha.
pub struct Signal;

#[derive(Clone)]
pub struct Signals {
    pub display: Entity<Signal>,
    pub clock: Entity<Signal>,
    pub queue: Entity<Signal>,
    pub entries: Entity<Signal>,
    pub volume: Entity<Signal>,
    pub error: Entity<Signal>,
}

impl Signals {
    fn new(cx: &mut App) -> Self {
        Self {
            display: cx.new(|_| Signal),
            clock: cx.new(|_| Signal),
            queue: cx.new(|_| Signal),
            entries: cx.new(|_| Signal),
            volume: cx.new(|_| Signal),
            error: cx.new(|_| Signal),
        }
    }
}

/// O `Wake` do controlador manda um aviso por este canal; o `Store` o transforma num tique.
pub fn wake_channel() -> (Wake, UnboundedReceiver<()>) {
    let (tx, rx) = unbounded();
    (
        Arc::new(move || {
            let _ = tx.unbounded_send(());
        }),
        rx,
    )
}

pub struct Store {
    controller: Controller,
    pub signals: Signals,
    selected: Option<u64>,
    last_current: Option<u64>,
    quit_requested: Arc<AtomicBool>,
    tasks: Vec<Task<()>>,
}

impl Store {
    pub fn new(player: Arc<dyn Player>, config: Config, cx: &mut Context<Self>) -> Self {
        Self {
            controller: Controller::new(player, config),
            signals: Signals::new(cx),
            selected: None,
            last_current: None,
            quit_requested: Arc::new(AtomicBool::new(false)),
            tasks: Vec::new(),
        }
    }

    /// Liga o poll de 10 Hz, como o `Timer` do SwiftUI, e o tique na hora quando uma resposta
    /// chega. O timer do GPUI é um `dispatch_after` sem folga; o do Swift tem 20 ms, e essa
    /// diferença fica registrada no resultado.
    pub fn start(&mut self, mut woken: UnboundedReceiver<()>, cx: &mut Context<Self>) {
        const POLL: Duration = Duration::from_millis(100);
        self.tasks.push(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                if this.update(cx, |store, cx| store.tick(cx)).is_err() {
                    break;
                }
            }
        }));
        self.tasks.push(cx.spawn(async move |this, cx| {
            while woken.next().await.is_some() {
                // Várias respostas juntas viram um tique só.
                while woken.try_recv().is_ok() {}
                if this.update(cx, |store, cx| store.tick(cx)).is_err() {
                    break;
                }
            }
        }));
    }

    pub fn model(&self) -> &Model {
        self.controller.model()
    }

    pub fn tick(&mut self, cx: &mut Context<Self>) {
        let changes = self.controller.tick();
        // A seleção acompanha a faixa atual, como o `onChange(of: currentItemID)` do SwiftUI.
        let current = self.controller.model().queue.current_item_id();
        let follows = current != self.last_current;
        if follows {
            self.last_current = current;
            self.selected = current;
        }
        self.publish(
            Changes {
                queue: changes.queue || follows,
                ..changes
            },
            cx,
        );
        if self.quit_requested.load(Ordering::Acquire) {
            cx.quit();
        }
    }

    fn publish(&self, changes: Changes, cx: &mut App) {
        let parts = [
            (changes.display, &self.signals.display),
            (changes.clock, &self.signals.clock),
            (changes.queue, &self.signals.queue),
            (changes.entries, &self.signals.entries),
            (changes.volume, &self.signals.volume),
            (changes.error, &self.signals.error),
        ];
        for (changed, signal) in parts {
            if changed {
                signal.update(cx, |_, cx| cx.notify());
            }
        }
    }

    fn notify(signal: &Entity<Signal>, cx: &mut App) {
        signal.update(cx, |_, cx| cx.notify());
    }

    pub fn selected(&self) -> Option<u64> {
        self.selected
    }

    pub fn toggle(&mut self) {
        self.controller.toggle();
    }

    pub fn next(&mut self) {
        self.controller.next();
    }

    pub fn previous(&mut self) {
        self.controller.previous();
    }

    pub fn clear(&mut self) {
        self.controller.clear();
    }

    pub fn add(&mut self, paths: Vec<PathBuf>) {
        self.controller.add(paths);
    }

    /// O clique numa linha seleciona e toca, como o `onTapGesture` do SwiftUI.
    pub fn play_row(&mut self, id: u64, cx: &mut Context<Self>) {
        self.selected = Some(id);
        self.controller.select(id);
        Self::notify(&self.signals.queue, cx);
    }

    pub fn select_row(&mut self, id: Option<u64>, cx: &mut Context<Self>) {
        self.selected = id;
        Self::notify(&self.signals.queue, cx);
    }

    /// Setas para cima e para baixo movem a seleção, como numa `List`.
    pub fn move_selection(&mut self, delta: i32, cx: &mut Context<Self>) {
        let items = &self.controller.model().queue.items;
        let Some(last) = items.len().checked_sub(1) else {
            return;
        };
        let position = self
            .selected
            .and_then(|id| items.iter().position(|t| t.id == id));
        let target = match position {
            Some(index) => (index as i64 + i64::from(delta)).clamp(0, last as i64) as usize,
            None if delta > 0 => 0,
            None => last,
        };
        self.selected = Some(items[target].id);
        Self::notify(&self.signals.queue, cx);
    }

    pub fn remove_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected.take() {
            self.controller.remove(id);
            Self::notify(&self.signals.queue, cx);
        }
    }

    /// Publica na hora: o slider não espera o core.
    pub fn apply_volume(&mut self, value: f64, cx: &mut Context<Self>) {
        self.controller.apply_volume(value.clamp(0.0, 1.0));
        Self::notify(&self.signals.volume, cx);
    }

    /// O passo do teclado e do VoiceOver.
    pub fn nudge_volume(&mut self, delta: f64, cx: &mut Context<Self>) {
        let value = self.controller.model().volume.value + delta;
        self.apply_volume(value, cx);
    }

    pub fn set_volume_editing(&mut self, editing: bool) {
        self.controller.set_volume_editing(editing);
    }

    pub fn quit_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.quit_requested)
    }

    /// Para o poll e devolve o device. O erro volta para quem chamou registrar em log: a janela
    /// já está fechando.
    pub fn shutdown(&mut self) -> Result<(), String> {
        self.tasks.clear();
        self.controller.shutdown()
    }

    /// Só para testes: espera os comandos em curso sem o poll.
    pub fn wait_idle(&mut self, timeout: Duration) -> bool {
        self.controller.wait_idle(timeout)
    }

    /// Só para testes: espera as linhas da fila carregarem.
    pub fn wait_entries(&mut self, timeout: Duration) -> bool {
        self.controller.wait_entries(timeout)
    }
}
