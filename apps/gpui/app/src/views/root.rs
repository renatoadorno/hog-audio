use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext, Context, Div, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, StyleRefinement, Styled, Subscription, Window, div, px,
    svg,
};
use hog_player_model::layout::{COLUMN_SPACING, PADDING, cover_side};

use super::clock::ClockView;
use super::cover::CoverView;
use super::queue::QueueView;
use super::status::StatusView;
use super::technical::TechnicalView;
use super::tooltip::tooltip;
use super::track_info::TrackInfoView;
use super::transport::TransportView;
use super::volume::VolumeView;
use super::{icons, observe};
use crate::actions::Open;
use crate::store::Store;
use crate::theme::*;

pub struct RootView {
    store: Entity<Store>,
    cover: Entity<CoverView>,
    track_info: Entity<TrackInfoView>,
    clock: Entity<ClockView>,
    transport: Entity<TransportView>,
    volume: Entity<VolumeView>,
    technical: Entity<TechnicalView>,
    status: Entity<StatusView>,
    queue: Entity<QueueView>,
    focus: FocusHandle,
    _error: Subscription,
}

/// Caching exige tamanho definido: a view em cache é posta em `style`, não medida pelo conteúdo.
fn sized(width: f32, height: f32) -> StyleRefinement {
    StyleRefinement::default().w(px(width)).h(px(height))
}

fn spacer(height: f32) -> Div {
    div().h(px(height)).flex_shrink_0()
}

impl RootView {
    pub fn new(store: Entity<Store>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _error = observe(&store, |s| &s.error, cx);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            cover: cx.new(|cx| CoverView::new(store.clone(), cx)),
            track_info: cx.new(|cx| TrackInfoView::new(store.clone(), cx)),
            clock: cx.new(|cx| ClockView::new(store.clone(), cx)),
            transport: cx.new(|cx| TransportView::new(store.clone(), cx)),
            volume: cx.new(|cx| VolumeView::new(store.clone(), window, cx)),
            technical: cx.new(|cx| TechnicalView::new(store.clone(), cx)),
            status: cx.new(|cx| StatusView::new(store.clone(), cx)),
            queue: cx.new(|cx| QueueView::new(store.clone(), cx)),
            store,
            focus,
            _error,
        }
    }

    fn left_column(&self, cover: f32) -> Div {
        div()
            .debug_selector(|| "coluna".to_string())
            .w(px(cover))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .child(self.cover.clone().cached(sized(cover, cover)))
            .child(spacer(GAP))
            .child(self.track_info.clone().cached(sized(cover, TITLE_BLOCK)))
            .child(spacer(GAP))
            .child(self.clock.clone().cached(sized(cover, CLOCK_HEIGHT)))
            .child(spacer(GAP))
            .child(
                self.transport
                    .clone()
                    .cached(sized(cover, TRANSPORT_HEIGHT)),
            )
            .child(spacer(GAP))
            .child(self.volume_slot(cover))
            .child(spacer(TECHNICAL_GAP))
            .child(
                self.technical
                    .clone()
                    .cached(sized(cover, TECHNICAL_HEIGHT)),
            )
    }

    fn volume_slot(&self, cover: f32) -> impl IntoElement {
        self.volume.clone().cached(sized(cover, VOLUME_HEIGHT))
    }

    fn queue_slot(&self, width: f32, height: f32) -> impl IntoElement {
        self.queue.clone().cached(sized(width, height))
    }

    fn title_bar(&self, palette: &Palette) -> Div {
        let hover = palette.fill;
        div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(TITLEBAR_HEIGHT))
            .flex()
            .items_center()
            .justify_end()
            .pr(px(16.0))
            .child(
                div()
                    .id("abrir")
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded(px(6.0))
                    .hover(move |s| s.bg(hover))
                    .text_size(px(13.0))
                    .child(
                        svg()
                            .data(icons::PLUS)
                            .size(px(14.0))
                            .text_color(palette.foreground),
                    )
                    .child("Abrir")
                    .tooltip(tooltip("Abrir arquivos ou pastas"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(Open), cx)),
            )
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let viewport = window.viewport_size();
        let width = f32::from(viewport.width);
        let height = f32::from(viewport.height) - TITLEBAR_HEIGHT;
        let cover = cover_side(width, height);
        let has_error = self.store.read(cx).model().error.is_some();
        let status = if has_error {
            STATUS_HEIGHT + STATUS_GAP
        } else {
            0.0
        };
        let queue_width = width - 2.0 * PADDING - COLUMN_SPACING - cover;
        let queue_height = height - 2.0 * PADDING - status;

        div()
            .id("raiz")
            .key_context("Player")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .font_family(FONT)
            .bg(palette.background)
            .text_color(palette.foreground)
            .child(self.title_bar(&palette))
            .child(
                div()
                    .absolute()
                    .top(px(TITLEBAR_HEIGHT))
                    .left_0()
                    .w(px(width))
                    .h(px(height))
                    .p(px(PADDING))
                    .flex()
                    .flex_row()
                    .gap(px(COLUMN_SPACING))
                    .child(self.left_column(cover))
                    .child(
                        div()
                            .w(px(queue_width))
                            .h_full()
                            .flex()
                            .flex_col()
                            .gap(px(STATUS_GAP))
                            // Sem erro não entra nada: um filho vazio ainda levaria o `gap`
                            // e desceria a fila.
                            .when(has_error, |c| {
                                c.child(
                                    self.status
                                        .clone()
                                        .cached(sized(queue_width, STATUS_HEIGHT)),
                                )
                            })
                            .child(self.queue_slot(queue_width, queue_height)),
                    ),
            )
    }
}
