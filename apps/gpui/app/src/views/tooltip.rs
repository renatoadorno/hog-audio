use gpui::{
    AnyView, App, AppContext, Context, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div, px,
};

use crate::theme::Palette;

pub struct Tooltip {
    text: SharedString,
    palette: Palette,
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .bg(self.palette.panel)
            .text_color(self.palette.foreground)
            .text_size(px(12.0))
            .child(self.text.clone())
    }
}

/// O construtor que o `.tooltip(...)` do GPUI pede: a dica é uma view.
pub fn tooltip(
    text: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |window, cx| {
        let palette = Palette::for_appearance(window.appearance());
        let text = text.clone();
        cx.new(move |_| Tooltip { text, palette }).into()
    }
}
