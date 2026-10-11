//! O slider do volume, adaptado do slider do Pawse (`crates/ui_components/src/slider.rs`, MIT,
//! © 2026 Aleksandr Popov). O arrasto segue a técnica do scrollbar do Zed: o movimento e o
//! mouse-up são ouvidos na janela inteira, na fase de captura, então soltar fora do trilho também
//! encerra o arrasto. Não há drag do GPUI: com um aberto, cada movimento do mouse redesenha a
//! janela inteira, sem cache.

use gpui::accesskit::ActionData;
use gpui::prelude::FluentBuilder;
use gpui::{
    AccessibleAction, Bounds, Context, DispatchPhase, Entity, FontWeight, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Point, Render, Role, StatefulInteractiveElement, Styled, Subscription, Window, canvas, div, px,
    relative, svg,
};

use super::tooltip::tooltip;
use super::{icons, observe};
use crate::store::Store;
use crate::theme::{CAPTION, Palette, tabular_digits};

/// Como no NSSlider, o botão fica dentro do trilho: o centro dele anda de um raio a um raio das
/// pontas, e é esse trecho que vai de 0 a 1.
const KNOB_RADIUS: f32 = 6.0;

/// O número que a tela mostra e o VoiceOver anuncia. Trunca, como o texto sempre fez: os dois
/// têm de dizer o mesmo.
pub fn percent(value: f64) -> i64 {
    (value * 100.0) as i64
}

pub struct VolumeView {
    store: Entity<Store>,
    track: Bounds<Pixels>,
    dragging: bool,
    _volume: Subscription,
    _activation: Subscription,
}

impl VolumeView {
    pub fn new(store: Entity<Store>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _volume = observe(&store, |s| &s.volume, cx);
        // O AppKit pode não entregar o mouse-up (⌘-Tab com o botão apertado), e o GPUI não
        // encerra nada na desativação: sem isto, o `editing` ficaria preso.
        let _activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.end(cx);
            }
        });
        Self {
            store,
            track: Bounds::default(),
            dragging: false,
            _volume,
            _activation,
        }
    }

    /// Com a mão no slider, o volume mudado por fora não o move (a regra do controlador).
    fn begin(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.dragging = true;
        self.store.update(cx, |s, _| s.set_volume_editing(true));
        self.seek(position, cx);
    }

    fn seek(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let start = self.track.left() + px(KNOB_RADIUS);
        let span = self.track.size.width - px(2.0 * KNOB_RADIUS);
        if span <= px(0.0) {
            return;
        }
        let value = f64::from(((position.x - start) / span).clamp(0.0, 1.0));
        // Com o botão apertado, o macOS reenvia o último movimento a cada 16 ms.
        if value == self.store.read(cx).model().volume.value {
            return;
        }
        self.store.update(cx, |s, cx| s.apply_volume(value, cx));
    }

    fn end(&mut self, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        self.store.update(cx, |s, _| s.set_volume_editing(false));
    }

    fn listen_to_window(entity: Entity<Self>, window: &mut Window) {
        let mover = entity.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && event.dragging() && mover.read(cx).dragging {
                mover.update(cx, |this, cx| this.seek(event.position, cx));
            }
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                entity.update(cx, |this, cx| this.end(cx));
            }
        });
    }
}

impl Render for VolumeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let volume = self.store.read(cx).model().volume;
        let fraction = volume.value as f32;
        let silent = volume.muted || volume.value < 0.01;
        let entity = cx.entity();

        div()
            .size_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .id("mudo")
                    .w(px(20.0))
                    .flex()
                    .justify_center()
                    .child(
                        svg()
                            .data(if silent {
                                icons::SPEAKER_SLASH
                            } else {
                                icons::SPEAKER
                            })
                            .size(px(16.0))
                            .text_color(palette.accent),
                    )
                    .when(volume.muted, |d| {
                        let message = "O device está no mudo";
                        d.role(Role::Image)
                            .aria_label(message)
                            .tooltip(tooltip(message))
                    }),
            )
            .child(
                div()
                    .id("volume")
                    .role(Role::Slider)
                    .aria_label("Volume")
                    .aria_numeric_value(percent(volume.value) as f64)
                    .aria_min_numeric_value(0.0)
                    .aria_max_numeric_value(100.0)
                    .aria_numeric_value_step(5.0)
                    .on_a11y_action(AccessibleAction::Increment, {
                        let store = self.store.clone();
                        move |_, _, cx| store.update(cx, |s, cx| s.nudge_volume(0.05, cx))
                    })
                    .on_a11y_action(AccessibleAction::Decrement, {
                        let store = self.store.clone();
                        move |_, _, cx| store.update(cx, |s, cx| s.nudge_volume(-0.05, cx))
                    })
                    .on_a11y_action(AccessibleAction::SetValue, {
                        let store = self.store.clone();
                        move |data, _, cx| {
                            if let Some(ActionData::NumericValue(value)) = data {
                                store.update(cx, |s, cx| s.apply_volume(value / 100.0, cx));
                            }
                        }
                    })
                    .debug_selector(|| "trilho-volume".to_string())
                    .flex_1()
                    .h(px(16.0))
                    .relative()
                    .flex()
                    .items_center()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.begin(event.position, cx)
                        }),
                    )
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
                                    .w(relative(fraction)),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(KNOB_RADIUS))
                            .right(px(KNOB_RADIUS))
                            .child(
                                div()
                                    .debug_selector(|| "botao-volume".to_string())
                                    .absolute()
                                    .top(px(2.0))
                                    .left(relative(fraction))
                                    .ml(px(-KNOB_RADIUS))
                                    .size(px(2.0 * KNOB_RADIUS))
                                    .rounded_full()
                                    .bg(palette.foreground),
                            ),
                    )
                    .child(
                        canvas(
                            {
                                let entity = entity.clone();
                                move |bounds, _, cx| {
                                    entity.update(cx, |this, _| this.track = bounds)
                                }
                            },
                            move |_, _, window, _| Self::listen_to_window(entity, window),
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .child(
                CAPTION
                    .apply(div())
                    .font_features(tabular_digits())
                    .w(px(40.0))
                    .flex()
                    .justify_end()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.secondary)
                    .child(format!("{}%", percent(volume.value))),
            )
    }
}
