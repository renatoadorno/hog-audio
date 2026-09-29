//! Atalhos e menus. O SwiftUI tem ⌘O como atalho do botão da barra e espaço no botão de tocar;
//! aqui os dois também entram no menu, e a medição 8 registra a diferença.

use gpui::{App, Entity, KeyBinding, Menu, MenuItem, PathPromptOptions, actions};

use crate::store::Store;

actions!(
    hog_player,
    [
        Quit,
        Hide,
        Minimize,
        Open,
        PlayPause,
        RemoveSelected,
        SelectPrevious,
        SelectNext
    ]
);

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("cmd-o", Open, None),
        KeyBinding::new("space", PlayPause, None),
        // Sem campo de texto na janela, o Delete vale em qualquer lugar. No SwiftUI ele só
        // funciona com a lista em foco: diferença deliberada, registrada na medição 7.
        KeyBinding::new("backspace", RemoveSelected, None),
        KeyBinding::new("delete", RemoveSelected, None),
        KeyBinding::new("up", SelectPrevious, None),
        KeyBinding::new("down", SelectNext, None),
    ]);
}

pub fn register(store: &Entity<Store>, cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &Minimize, cx| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_, window, _| window.minimize_window());
        }
    });
    let s = store.clone();
    cx.on_action(move |_: &PlayPause, cx| s.update(cx, |store, _| store.toggle()));
    let s = store.clone();
    cx.on_action(move |_: &RemoveSelected, cx| s.update(cx, |store, cx| store.remove_selected(cx)));
    let s = store.clone();
    cx.on_action(move |_: &SelectPrevious, cx| {
        s.update(cx, |store, cx| store.move_selection(-1, cx))
    });
    let s = store.clone();
    cx.on_action(move |_: &SelectNext, cx| s.update(cx, |store, cx| store.move_selection(1, cx)));
    let s = store.clone();
    cx.on_action(move |_: &Open, cx| open_panel(&s, cx));
}

/// O NSOpenPanel do GPUI não filtra por tipo: o filtro fica para o `audio_files` do controlador.
fn open_panel(store: &Entity<Store>, cx: &mut App) {
    let paths = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: true,
        multiple: true,
        prompt: Some("Abrir".into()),
    });
    let store = store.clone();
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = paths.await {
            cx.update(|cx| store.update(cx, |store, _| store.add(paths)));
        }
    })
    .detach();
}

pub fn menus() -> Vec<Menu> {
    vec![
        // Os nomes que o macOS em pt-BR dá aos itens padrão do menu do app, que é o que o
        // SwiftUI mostra (ele não declara `commands`).
        Menu::new("hog-audio GPUI").items([
            MenuItem::action("Ocultar hog-audio GPUI", Hide),
            MenuItem::separator(),
            MenuItem::action("Encerrar hog-audio GPUI", Quit),
        ]),
        Menu::new("Arquivo").items([MenuItem::action("Abrir…", Open)]),
        Menu::new("Controles").items([
            MenuItem::action("Tocar ou pausar", PlayPause),
            MenuItem::action("Remover da fila", RemoveSelected),
        ]),
        Menu::new("Janela").items([MenuItem::action("Minimizar", Minimize)]),
    ]
}
