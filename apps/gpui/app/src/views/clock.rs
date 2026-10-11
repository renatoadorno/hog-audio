use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div, px,
    relative,
};

use super::observe;
use crate::store::Store;
use crate::theme::{CAPTION, Palette, tabular_digits};

/// Lê só o relógio: é o único trecho da tela que muda a cada tique enquanto toca.
pub struct ClockView {
    store: Entity<Store>,
    _clock: Subscription,
}

impl ClockView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _clock = observe(&store, |s| &s.clock, cx);
        Self { store, _clock }
    }
}

impl Render for ClockView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let clock = self.store.read(cx).model().clock.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .w_full()
                    .h(px(4.0))
                    .rounded_full()
                    .bg(palette.fill)
                    .child(
                        div()
                            .h_full()
                            .rounded_full()
                            .bg(palette.accent)
                            .w(relative(clock.progress as f32)),
                    ),
            )
            .child(
                CAPTION
                    .apply(div())
                    .font_features(tabular_digits())
                    .flex()
                    .justify_between()
                    .text_color(palette.secondary)
                    .child(clock.elapsed)
                    .child(clock.total),
            )
    }
}
