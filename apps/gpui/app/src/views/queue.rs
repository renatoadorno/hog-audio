use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, Render, RenderImage, ScrollStrategy, StatefulInteractiveElement, Styled,
    StyledImage, Subscription, UniformListScrollHandle, Window, div, img, px, rgba, svg,
    uniform_list,
};
use hog_player_model::entry::QueueEntryInfo;
use hog_player_model::media::tags::file_stem;

use super::tooltip::tooltip;
use super::{icons, observe};
use crate::images::ThumbnailCache;
use crate::store::Store;
use crate::theme::{
    BODY, CAPTION, CAPTION2, Palette, ROW_HEIGHT, SUBHEADLINE, THUMB, TITLE2, tabular_digits,
};

/// O que uma linha visível precisa, copiado do modelo antes de desenhar: a conversão das
/// miniaturas precisa de `&mut App`, que não convive com o empréstimo do `Store`.
struct Row {
    id: u64,
    path: String,
    failure: Option<String>,
    entry: Option<QueueEntryInfo>,
    is_current: bool,
    is_playing: bool,
    selected: bool,
}

pub struct QueueView {
    store: Entity<Store>,
    scroll: UniformListScrollHandle,
    thumbnails: ThumbnailCache,
    followed_current: Option<u64>,
    followed_selection: Option<u64>,
    prune: bool,
    _queue: Subscription,
    _entries: Subscription,
}

impl QueueView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _queue = observe(&store, |s| &s.queue, cx);
        let entries = store.read(cx).signals.entries.clone();
        let _entries = cx.observe(&entries, |this, _, cx| {
            this.prune = true;
            cx.notify();
        });
        Self {
            store,
            scroll: UniformListScrollHandle::new(),
            thumbnails: ThumbnailCache::default(),
            followed_current: None,
            followed_selection: None,
            prune: false,
            _queue,
            _entries,
        }
    }

    /// Rola até a faixa atual quando ela muda, e até a seleção quando as setas a movem.
    fn follow(&mut self, cx: &Context<Self>) {
        let store = self.store.read(cx);
        let queue = &store.model().queue;
        let current = queue.current_item_id();
        let selected = store.selected();
        let target = if current != self.followed_current {
            self.followed_current = current;
            current
        } else if selected != self.followed_selection {
            selected
        } else {
            None
        };
        self.followed_selection = selected;
        if let Some(index) = target.and_then(|id| queue.items.iter().position(|t| t.id == id)) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        }
    }

    fn rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let palette = Palette::for_appearance(window.appearance());
        let rows: Vec<Row> = {
            let store = self.store.read(cx);
            let model = store.model();
            let queue = &model.queue;
            range
                .filter_map(|index| {
                    queue.items.get(index).map(|track| Row {
                        id: track.id,
                        path: track.path.clone(),
                        failure: track.failure.clone(),
                        entry: model.entries.get(&track.path).cloned(),
                        is_current: queue.current_index == Some(index as u32),
                        is_playing: queue.is_playing,
                        selected: store.selected() == Some(track.id),
                    })
                })
                .collect()
        };
        rows.into_iter()
            .map(|row| {
                let thumbnail = row
                    .entry
                    .as_ref()
                    .and_then(|entry| entry.thumbnail.as_ref())
                    .and_then(|thumbnail| self.thumbnails.get(&row.path, thumbnail, cx));
                self.row(row, thumbnail, &palette, cx)
            })
            .collect()
    }

    fn row(
        &self,
        row: Row,
        thumbnail: Option<Arc<RenderImage>>,
        palette: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let problem = row
            .failure
            .clone()
            .or_else(|| row.entry.as_ref().and_then(|e| e.problem.clone()));
        let title = row
            .entry
            .as_ref()
            .map_or_else(|| file_stem(&row.path), |e| e.title.clone());
        let subtitle = row
            .entry
            .as_ref()
            .map(QueueEntryInfo::subtitle)
            .unwrap_or_default();
        let (hover, selected_bg, id) = (palette.fill, palette.accent_soft, row.id);
        div()
            .id(("faixa", row.id))
            .debug_selector(move || format!("linha-{id}"))
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(8.0))
            .rounded(px(8.0))
            .when(row.selected, |d| d.bg(selected_bg))
            .hover(move |s| s.bg(hover))
            .opacity(if problem.is_some() { 0.5 } else { 1.0 })
            .child(thumb_cell(
                thumbnail,
                row.is_current,
                row.is_playing,
                palette,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        BODY.apply(div())
                            .when(row.is_current, |d| d.font_weight(FontWeight::SEMIBOLD))
                            .truncate()
                            .child(title),
                    )
                    .when(!subtitle.is_empty(), |d| {
                        d.child(
                            SUBHEADLINE
                                .apply(div())
                                .text_color(palette.secondary)
                                .truncate()
                                .child(subtitle),
                        )
                    }),
            )
            .child(trailing(&row, problem.is_some(), palette))
            .when_some(problem, |d, problem| d.tooltip(tooltip(problem)))
            .on_click(
                cx.listener(move |this, _, _, cx| {
                    this.store.update(cx, |s, cx| s.play_row(id, cx))
                }),
            )
            .into_any_element()
    }
}

fn thumb_cell(
    image: Option<Arc<RenderImage>>,
    is_current: bool,
    is_playing: bool,
    palette: &Palette,
) -> Div {
    div()
        .size(px(THUMB))
        .flex_shrink_0()
        .relative()
        .rounded(px(6.0))
        .overflow_hidden()
        .bg(palette.fill)
        .flex()
        .items_center()
        .justify_center()
        .map(|d| match image {
            Some(image) => d.child(
                img(image)
                    .size_full()
                    .rounded(px(6.0))
                    .object_fit(ObjectFit::Cover),
            ),
            None => d.child(
                svg()
                    .data(icons::MUSIC_NOTE)
                    .size(px(18.0))
                    .text_color(palette.secondary),
            ),
        })
        .when(is_current, |d| {
            // A máscara de conteúdo do GPUI é retangular: sem o próprio arredondamento, o véu
            // escuro passaria dos cantos da miniatura.
            d.child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(6.0))
                    .bg(rgba(0x00000073))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .data(if is_playing {
                                icons::WAVEFORM
                            } else {
                                icons::SPEAKER
                            })
                            .size(px(16.0))
                            .text_color(palette.accent),
                    ),
            )
        })
}

fn trailing(row: &Row, has_problem: bool, palette: &Palette) -> Div {
    let mut trailing = div().flex().items_center().gap(px(8.0)).flex_shrink_0();
    if has_problem {
        trailing = trailing.child(
            svg()
                .data(icons::WARNING)
                .size(px(16.0))
                .text_color(palette.warning),
        );
    } else if let Some(entry) = &row.entry
        && let Some(quality) = &entry.quality
    {
        trailing = trailing.child(
            CAPTION2
                .apply(div())
                .id(("selo", row.id))
                .px(px(8.0))
                .py(px(2.0))
                .rounded_full()
                .bg(palette.fill)
                .font_features(tabular_digits())
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.secondary)
                .child(quality.clone())
                .when_some(entry.quality_detail.clone(), |badge, detail| {
                    badge.tooltip(tooltip(detail))
                }),
        );
    }
    if let Some(duration) = row.entry.as_ref().and_then(|e| e.duration.clone()) {
        trailing = trailing.child(
            CAPTION
                .apply(div())
                .font_features(tabular_digits())
                .min_w(px(36.0))
                .flex()
                .justify_end()
                .text_color(palette.secondary)
                .child(duration),
        );
    }
    trailing
}

impl Render for QueueView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        if std::mem::take(&mut self.prune) {
            let keep: HashSet<String> = self
                .store
                .read(cx)
                .model()
                .entries
                .keys()
                .cloned()
                .collect();
            self.thumbnails.retain(|path| keep.contains(path), cx);
        }
        self.follow(cx);
        let count = self.store.read(cx).model().queue.items.len();
        let empty = count == 0;
        let clear_color = if empty {
            palette.secondary
        } else {
            palette.accent
        };

        let header = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(4.0))
            .child(
                CAPTION
                    .apply(div())
                    .font_weight(FontWeight::BOLD)
                    .text_color(palette.secondary)
                    .child("FILA"),
            )
            .child(
                CAPTION2
                    .apply(div())
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded_full()
                    .bg(palette.accent_soft)
                    .font_features(tabular_digits())
                    .font_weight(FontWeight::BOLD)
                    .text_color(palette.accent)
                    .child(count.to_string()),
            )
            .child(div().flex_1())
            .child(
                CAPTION
                    .apply(div())
                    .id("limpar")
                    .debug_selector(|| "limpar".to_string())
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(clear_color)
                    .child("Limpar")
                    .when(!empty, |b| {
                        b.on_click(
                            cx.listener(|this, _, _, cx| this.store.update(cx, |s, _| s.clear())),
                        )
                    }),
            );

        let body = if empty {
            div()
                .debug_selector(|| "fila-vazia".to_string())
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(
                    svg()
                        .data(icons::LIST)
                        .size(px(TITLE2.size))
                        .text_color(palette.accent),
                )
                .child(
                    SUBHEADLINE
                        .apply(div())
                        .text_color(palette.secondary)
                        .child("Arraste músicas ou uma pasta para começar"),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h(px(0.0))
                .child(
                    uniform_list(
                        "fila",
                        count,
                        cx.processor(|this, range: Range<usize>, window, cx| {
                            this.rows(range, window, cx)
                        }),
                    )
                    .track_scroll(&self.scroll)
                    .size_full(),
                )
                .into_any_element()
        };

        div()
            .debug_selector(|| "fila".to_string())
            .size_full()
            .p(px(12.0))
            .rounded(px(12.0))
            .bg(palette.panel)
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(header)
            .child(body)
    }
}
