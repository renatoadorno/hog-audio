//! Atalhos e menus. O SwiftUI tem ⌘O como atalho do botão da barra e espaço no botão de tocar;
//! aqui os dois também entram no menu, e a medição 8 registra a diferença.

use gpui::{App, Entity, KeyBinding, Menu, MenuItem, PathPromptOptions, Window, actions};

use crate::store::Store;

actions!(
    hog_player,
    [
        Quit,
        Hide,
        Minimize,
        Close,
        Open,
        PlayPause,
        RemoveSelected,
        SelectPrevious,
        SelectNext
    ]
);

#[cfg(feature = "panic-probe")]
actions!(hog_player_debug, [ProvokePanic]);

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("cmd-w", Close, None),
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
    cx.on_action(|_: &Minimize, cx| on_active_window(cx, |window| window.minimize_window()));
    cx.on_action(|_: &Close, cx| on_active_window(cx, |window| window.remove_window()));
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
    #[cfg(feature = "panic-probe")]
    cx.on_action(|_: &ProvokePanic, _| panic!("pânico provocado pela medição 9"));
}

/// O handler global roda com a janela emprestada ao dispatch, e o `update` dela daria "window not
/// found": a ação fica para depois, como o `close_global` do workspace do Zed.
fn on_active_window(cx: &mut App, action: impl FnOnce(&mut Window) + 'static) {
    cx.defer(move |cx| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_, window, _| action(window));
        }
    });
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
    let menus = vec![
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
        Menu::new("Janela").items([
            MenuItem::action("Minimizar", Minimize),
            MenuItem::action("Fechar", Close),
        ]),
    ];
    // Sem a feature, um `let mut` com o push condicional vira `unused_mut` no clippy.
    #[cfg(feature = "panic-probe")]
    let menus = {
        let mut menus = menus;
        menus.push(Menu::new("Depurar").items([MenuItem::action("Provocar pânico", ProvokePanic)]));
        menus
    };
    menus
}
