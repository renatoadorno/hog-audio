use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
};

use super::observe;
use crate::store::Store;
use crate::theme::{CAPTION2, MONO_FONT, Palette};

pub struct TechnicalView {
    store: Entity<Store>,
    _display: Subscription,
}

impl TechnicalView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _display = observe(&store, |s| &s.display, cx);
        Self { store, _display }
    }
}

impl Render for TechnicalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let line = self.store.read(cx).model().display.technical_line.clone();
        CAPTION2
            .apply(div())
            .size_full()
            .font_family(MONO_FONT)
            .text_color(palette.secondary)
            .truncate()
            .child(line)
    }
}
