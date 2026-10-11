use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement, Render, Role,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};

use super::observe;
use crate::store::Store;
use crate::theme::{HEADLINE, Palette, SUBHEADLINE, TITLE2};

pub struct TrackInfoView {
    store: Entity<Store>,
    _display: Subscription,
}

impl TrackInfoView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _display = observe(&store, |s| &s.display, cx);
        Self { store, _display }
    }
}

impl Render for TrackInfoView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let display = self.store.read(cx).model().display.clone();
        let label = [&display.title, &display.artist, &display.album]
            .into_iter()
            .filter(|part| !part.is_empty())
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        div()
            .id("faixa-atual")
            .role(Role::Label)
            .aria_value(label)
            .size_full()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .overflow_hidden()
            .child(
                TITLE2
                    .apply(div())
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .child(display.title),
            )
            .when(!display.artist.is_empty(), |d| {
                // O `.headline` do macOS é negrito.
                d.child(
                    HEADLINE
                        .apply(div())
                        .font_weight(FontWeight::BOLD)
                        .text_color(palette.secondary)
                        .truncate()
                        .child(display.artist),
                )
            })
            .when(!display.album.is_empty(), |d| {
                d.child(
                    SUBHEADLINE
                        .apply(div())
                        .text_color(palette.tertiary)
                        .truncate()
                        .child(display.album),
                )
            })
    }
}
