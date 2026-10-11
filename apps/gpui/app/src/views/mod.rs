pub mod backdrop;
pub mod clock;
pub mod cover;
pub mod icons;
pub mod queue;
pub mod root;
pub mod status;
pub mod technical;
pub mod tooltip;
pub mod track_info;
pub mod transport;
pub mod volume;

use gpui::{Context, Entity, Subscription};

use crate::store::{Signal, Signals, Store};

/// Cada view observa só o sinal da parte do modelo que ela lê.
pub(crate) fn observe<V: 'static>(
    store: &Entity<Store>,
    pick: fn(&Signals) -> &Entity<Signal>,
    cx: &mut Context<V>,
) -> Subscription {
    let signal = pick(&store.read(cx).signals).clone();
    cx.observe(&signal, |_, _, cx| cx.notify())
}
