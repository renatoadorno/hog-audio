use std::sync::Arc;

use gpui::{
    Context, Div, Entity, InteractiveElement, IntoElement, ObjectFit, ParentElement, Render,
    RenderImage, Role, StatefulInteractiveElement, Styled, StyledImage, Subscription, Window, div,
    img, linear_color_stop, linear_gradient, px, rgb, rgba, svg,
};

use super::{icons, observe};
use crate::images::ArtworkSlot;
use crate::store::Store;
use crate::theme::Palette;

/// A capa da faixa atual, convertida uma vez por capa. Até a nova chegar, o controlador mantém a
/// da faixa anterior na tela, em vez de piscar o placeholder.
pub struct CoverView {
    store: Entity<Store>,
    slot: ArtworkSlot,
    _display: Subscription,
}

impl CoverView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _display = observe(&store, |s| &s.display, cx);
        Self {
            store,
            slot: ArtworkSlot::default(),
            _display,
        }
    }
}

fn placeholder(palette: &Palette) -> Div {
    div()
        .debug_selector(|| "capa-vazia".to_string())
        .size_full()
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

impl Render for CoverView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let artwork = self.store.read(cx).model().display.artwork.clone();
        let (image, gone): (Option<Arc<RenderImage>>, _) =
            self.slot.update(artwork.as_ref(), |art| Some(&art.cover));
        // Com a janela em mãos, a soltura vai direto ao atlas dela; o `App::drop_image` não a
        // alcançaria durante o desenho (ver `images.rs`).
        if let Some(gone) = gone {
            let _ = window.drop_image(gone);
        }
        let content = match image {
            Some(image) => img(image)
                .debug_selector(|| "capa-imagem".to_string())
                .size_full()
                .rounded(px(12.0))
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => placeholder(&palette).into_any_element(),
        };
        div()
            .id("capa")
            .role(Role::Image)
            .aria_label("Capa")
            .size_full()
            .rounded(px(12.0))
            .overflow_hidden()
            .border_1()
            .border_color(rgba(0xffffff1f))
            .shadow_lg()
            .child(content)
    }
}
