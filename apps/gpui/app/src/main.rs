use std::sync::Arc;

use gpui::{
    App, AppContext, Bounds, Entity, QuitMode, TitlebarOptions, WindowBounds, WindowOptions, point,
    px, size,
};
use hog_audio::api::HogPlayer;
use hog_player_gpui::actions;
use hog_player_gpui::store::{Store, wake_channel};
use hog_player_gpui::theme::TITLEBAR_HEIGHT;
use hog_player_gpui::views::root::RootView;
use hog_player_model::controller::{Config, LogSink};
use hog_player_model::layout::{DEFAULT_WINDOW, MIN_WINDOW};

fn main() {
    // Criado antes de qualquer janela, como o `PlayerViewModel` no AppDelegate: não toma device
    // nenhum até o primeiro play.
    let player = HogPlayer::new();
    gpui_platform::application()
        // Fechar a última janela encerra, como `applicationShouldTerminateAfterLastWindowClosed`.
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx: &mut App| {
            let (wake, woken) = wake_channel();
            let config = Config::production(transition_log(), wake);
            let store = cx.new(|cx| Store::new(player.clone(), config, cx));
            store.update(cx, |store, cx| store.start(woken, cx));
            actions::bind(cx);
            actions::register(&store, cx);
            cx.set_menus(actions::menus());
            open_window(&store, cx);
            cx.activate(true);
            // Um arquivo pela linha de comando, como no SwiftUI: itera rápido sem o painel.
            if let Some(path) = std::env::args().nth(1) {
                store.update(cx, |store, _| store.add(vec![path.into()]));
            }
        });
}

/// O log de transições vai para stderr; a Task 9 acrescenta o arquivo.
fn transition_log() -> LogSink {
    Arc::new(|line: &str| eprintln!("{line}"))
}

fn open_window(store: &Entity<Store>, cx: &mut App) {
    let (width, height) = DEFAULT_WINDOW;
    let (min_width, min_height) = MIN_WINDOW;
    let bounds = Bounds::centered(None, size(px(width), px(height + TITLEBAR_HEIGHT)), cx);
    let store = store.clone();
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("hog-audio GPUI".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(12.0), px(14.0))),
            }),
            window_min_size: Some(size(px(min_width), px(min_height + TITLEBAR_HEIGHT))),
            app_id: Some("local.hogaudio.gpui".to_string()),
            ..Default::default()
        },
        move |window, cx| cx.new(|cx| RootView::new(store, window, cx)),
    )
    .expect("não consegui abrir a janela");
}
