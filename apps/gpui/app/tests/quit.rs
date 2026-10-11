//! A saída pelo GPUI: sair do app devolve o device, o ⌘W fecha a janela e o ⌘M a minimiza. Sem
//! panic hook aqui: ele é global ao processo e mora no binário de `tests/exit.rs`.

use std::sync::Arc;

use gpui::{AppContext, Entity, TestAppContext};
use hog_player_gpui::store::Store;
use hog_player_gpui::views::root::RootView;
use hog_player_gpui::{actions, exit};
use hog_player_testing::{FakePlayer, config_de_teste, sem_log};

fn store(cx: &mut TestAppContext, fake: &Arc<FakePlayer>) -> Entity<Store> {
    let player = fake.clone();
    cx.new(move |cx| Store::new(player, config_de_teste(), cx))
}

/// O `TestAppContext::quit` roda o `App::shutdown`, que chama os observadores do `on_app_quit`:
/// é o mesmo caminho que o macOS percorre no ⌘Q, no botão vermelho da última janela e no sinal.
#[gpui::test]
fn sair_do_app_devolve_o_device(cx: &mut TestAppContext) {
    let fake = FakePlayer::new();
    let store = store(cx, &fake);
    cx.update(|cx| exit::shutdown_on_quit(&store, sem_log(), cx));
    assert!(!fake.chamou("shutdown"));

    cx.quit();

    assert!(fake.chamou("shutdown"));
}

#[gpui::test]
fn cmd_w_fecha_a_janela(cx: &mut TestAppContext) {
    let fake = FakePlayer::new();
    let store = store(cx, &fake);
    cx.update(|cx| {
        actions::bind(cx);
        actions::register(&store, cx);
    });
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));
    // A janela de teste não nasce ativa, e o Fechar age sobre a janela ativa.
    window.update(|window, _| window.activate_window());
    assert_eq!(window.windows().len(), 1);

    window.simulate_keystrokes("cmd-w");
    window.run_until_parked();

    assert!(window.windows().is_empty());
}

/// A `TestWindow` não guarda estado de minimizada: o `minimize` dela é `unimplemented!()`. O
/// pânico é a prova de que o ⌘M chegou à plataforma. Se o GPUI implementar o `minimize` de
/// teste, este teste reprova e passa a poder olhar o estado.
#[gpui::test]
#[should_panic(expected = "not implemented")]
fn cmd_m_chega_a_plataforma(cx: &mut TestAppContext) {
    let fake = FakePlayer::new();
    let store = store(cx, &fake);
    cx.update(|cx| {
        actions::bind(cx);
        actions::register(&store, cx);
    });
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));
    window.update(|window, _| window.activate_window());

    window.simulate_keystrokes("cmd-m");
    window.run_until_parked();
}
