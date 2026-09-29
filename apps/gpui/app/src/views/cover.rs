use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
    linear_color_stop, linear_gradient, px, rgb, rgba, svg,
};

use super::{icons, observe};
use crate::store::Store;
use crate::theme::Palette;

pub struct CoverView {
    #[expect(
        dead_code,
        reason = "a capa de verdade (Task 8) lê a arte do modelo por ele"
    )]
    store: Entity<Store>,
    _display: Subscription,
}

impl CoverView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _display = observe(&store, |s| &s.display, cx);
        Self { store, _display }
    }
}

impl Render for CoverView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        div()
            .size_full()
            .rounded(px(12.0))
            .border_1()
            .border_color(rgba(0xffffff1f))
            .bg(linear_gradient(
                135.0,
                linear_color_stop(palette.accent, 0.0),
                linear_color_stop(rgb(0xb82e05), 1.0),
            ))
            .flex()
            .items_center()
            .justify_center()
            .child(
                svg()
                    .data(icons::WAVEFORM)
                    .size(px(64.0))
                    .text_color(rgba(0xffffffeb)),
            )
    }
}
