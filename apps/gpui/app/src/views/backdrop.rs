//! O fundo da janela: a capa da faixa atual já desfocada, sob um véu da cor da janela que mantém
//! o texto legível. Na troca de faixa, as duas se cruzam em 400 ms, o `easeInOut(0.4)` do
//! SwiftUI. A imagem vem pronta do controlador; a técnica de desfocar na CPU vem do Pawse.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Entity, IntoElement, ObjectFit, ParentElement, Render, RenderImage, Styled,
    StyledImage, Subscription, Window, div, ease_in_out, img, linear_color_stop, linear_gradient,
};

use super::observe;
use crate::images::ArtworkSlot;
use crate::store::Store;
use crate::theme::Palette;

const FADE: Duration = Duration::from_millis(400);

pub struct BackdropView {
    store: Entity<Store>,
    slot: ArtworkSlot,
    current: Option<Arc<RenderImage>>,
    previous: Option<Arc<RenderImage>>,
    fade_started: Option<Instant>,
    _display: Subscription,
}

impl BackdropView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _display = observe(&store, |s| &s.display, cx);
        Self {
            store,
            slot: ArtworkSlot::default(),
            current: None,
            previous: None,
            fade_started: None,
            _display,
        }
    }
}

impl Render for BackdropView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let artwork = self.store.read(cx).model().display.artwork.clone();
        let (image, gone) = self
            .slot
            .update(artwork.as_ref(), |art| art.backdrop.as_ref());
        if gone.is_some() || (image.is_some() && self.current.is_none()) {
            if let Some(stale) = self.previous.take() {
                let _ = window.drop_image(stale);
            }
            self.previous = gone;
            self.current = image;
            self.fade_started = Some(Instant::now());
        }

        let progress = self.fade_started.map_or(1.0, |started| {
            ease_in_out((started.elapsed().as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0))
        });
        if progress < 1.0 {
            // Anima só durante o cruzamento; parado, o fundo não pede quadro nenhum.
            window.request_animation_frame();
        } else {
            self.fade_started = None;
            if let Some(stale) = self.previous.take() {
                let _ = window.drop_image(stale);
            }
        }

        let has_image = self.current.is_some() || self.previous.is_some();
        div()
            .size_full()
            .relative()
            .bg(linear_gradient(
                180.0,
                linear_color_stop(palette.background, 0.0),
                linear_color_stop(palette.accent_soft, 1.0),
            ))
            .when_some(self.previous.clone(), |d, image| {
                d.child(
                    img(image)
                        .absolute()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .opacity(1.0 - progress),
                )
            })
            .when_some(self.current.clone(), |d, image| {
                d.child(
                    img(image)
                        .absolute()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .opacity(progress),
                )
            })
            .when(has_image, |d| {
                d.child(div().absolute().size_full().bg(palette.veil))
            })
    }
}
