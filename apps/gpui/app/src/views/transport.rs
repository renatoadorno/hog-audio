use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Div, Entity, InteractiveElement, IntoElement, ParentElement, Render, Rgba, Stateful,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px, svg,
};

use super::{icons, observe};
use crate::store::Store;
use crate::theme::Palette;

pub struct TransportView {
    store: Entity<Store>,
    _display: Subscription,
}

impl TransportView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _display = observe(&store, |s| &s.display, cx);
        Self { store, _display }
    }
}

/// A opacidade de botão desabilitado do SwiftUI: 0,32 nos laterais e 0,4 no de tocar
/// (`ContentView.swift`, `transportButton` e o botão de tocar).
const SIDE_DIMMED: f32 = 0.32;
const TOGGLE_DIMMED: f32 = 0.4;

fn round_button(
    id: &'static str,
    icon: &'static [u8],
    diameter: f32,
    bg: Rgba,
    fg: Rgba,
    enabled: bool,
    dimmed: f32,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(diameter))
        .flex_shrink_0()
        .rounded_full()
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .opacity(if enabled { 1.0 } else { dimmed })
        .child(svg().data(icon).size(px(diameter * 0.4)).text_color(fg))
}

impl Render for TransportView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let display = self.store.read(cx).model().display.clone();
        let can_toggle = display.can_play || display.can_pause;
        let toggle_icon = if display.is_playing {
            icons::PAUSE
        } else {
            icons::PLAY
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(24.0))
            .child(
                round_button(
                    "anterior",
                    icons::BACKWARD,
                    40.0,
                    palette.fill,
                    palette.foreground,
                    display.can_go_previous,
                    SIDE_DIMMED,
                )
                .when(display.can_go_previous, |b| {
                    b.on_click(
                        cx.listener(|this, _, _, cx| this.store.update(cx, |s, _| s.previous())),
                    )
                }),
            )
            .child(
                round_button(
                    "tocar",
                    toggle_icon,
                    56.0,
                    palette.accent,
                    palette.on_accent,
                    can_toggle,
                    TOGGLE_DIMMED,
                )
                .when(can_toggle, |b| {
                    b.on_click(
                        cx.listener(|this, _, _, cx| this.store.update(cx, |s, _| s.toggle())),
                    )
                }),
            )
            .child(
                round_button(
                    "proxima",
                    icons::FORWARD,
                    40.0,
                    palette.fill,
                    palette.foreground,
                    display.can_go_next,
                    SIDE_DIMMED,
                )
                .when(display.can_go_next, |b| {
                    b.on_click(cx.listener(|this, _, _, cx| this.store.update(cx, |s, _| s.next())))
                }),
            )
    }
}
