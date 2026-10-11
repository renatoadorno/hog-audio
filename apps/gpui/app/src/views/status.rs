use gpui::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Role,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px, svg,
};

use super::{icons, observe};
use crate::store::Store;
use crate::theme::{CAPTION, Palette};

pub struct StatusView {
    store: Entity<Store>,
    _error: Subscription,
}

impl StatusView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _error = observe(&store, |s| &s.error, cx);
        Self { store, _error }
    }
}

impl Render for StatusView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        // Sem papel, o raiz vazio não gera nó: o `id` só iguala o tipo dos dois ramos.
        let Some(message) = self.store.read(cx).model().error.clone() else {
            return div().id("aviso");
        };
        CAPTION
            .apply(div())
            .id("aviso")
            .role(Role::Alert)
            .aria_label(message.clone())
            .size_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(12.0))
            .rounded(px(8.0))
            .bg(palette.error_soft)
            .text_color(palette.error)
            .child(
                svg()
                    .data(icons::WARNING)
                    .size(px(14.0))
                    .flex_shrink_0()
                    .text_color(palette.error),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .line_clamp(2)
                    .child(message),
            )
    }
}
