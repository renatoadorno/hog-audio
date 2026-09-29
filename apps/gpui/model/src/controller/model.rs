use std::collections::HashMap;

use crate::display::{ClockState, DisplayState};
use crate::entry::QueueEntryInfo;
use crate::port::Track;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueueState {
    pub items: Vec<Track>,
    pub current_index: Option<u32>,
    pub is_playing: bool,
}

impl QueueState {
    pub fn current(&self) -> Option<&Track> {
        self.current_index
            .and_then(|index| self.items.get(index as usize))
    }

    /// A linha da faixa atual: é por ela que a fila marca a seleção e rola até a atual.
    pub fn current_item_id(&self) -> Option<u64> {
        self.current().map(|track| track.id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeState {
    pub value: f64,
    pub muted: bool,
}

/// O que a tela lê, dividido como no `@Observable` do Swift: cada parte muda sozinha, e o
/// `tick()` diz quais mudaram para a UI redesenhar só elas.
#[derive(Clone, Debug)]
pub struct Model {
    pub display: DisplayState,
    pub clock: ClockState,
    pub queue: QueueState,
    /// Por caminho, como no Swift: chega aos poucos depois que a faixa entra na fila; até lá a
    /// linha mostra o nome do arquivo.
    pub entries: HashMap<String, QueueEntryInfo>,
    pub volume: VolumeState,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub display: bool,
    pub clock: bool,
    pub queue: bool,
    pub entries: bool,
    pub volume: bool,
    pub error: bool,
}

impl Changes {
    pub const NONE: Self = Self {
        display: false,
        clock: false,
        queue: false,
        entries: false,
        volume: false,
        error: false,
    };

    pub fn any(self) -> bool {
        self != Self::NONE
    }
}

/// O que o tique compara antes e depois. A fila e as linhas não entram: comparar mil faixas a
/// cada tique seria caro, e quem as muda já sabe que mudou.
pub(crate) struct Observed {
    display: DisplayState,
    clock: ClockState,
    current_index: Option<u32>,
    is_playing: bool,
    volume: VolumeState,
    error: Option<String>,
}

impl Observed {
    pub(crate) fn capture(model: &Model) -> Self {
        Self {
            display: model.display.clone(),
            clock: model.clock.clone(),
            current_index: model.queue.current_index,
            is_playing: model.queue.is_playing,
            volume: model.volume,
            error: model.error.clone(),
        }
    }

    pub(crate) fn changes(
        self,
        model: &Model,
        items_changed: bool,
        entries_changed: bool,
    ) -> Changes {
        Changes {
            display: self.display != model.display,
            clock: self.clock != model.clock,
            queue: items_changed
                || self.current_index != model.queue.current_index
                || self.is_playing != model.queue.is_playing,
            entries: entries_changed,
            volume: self.volume != model.volume,
            error: self.error != model.error,
        }
    }
}
