//! Cliques rápidos em Próxima pela janela inteira: o caminho do evento do GPUI até o controlador,
//! com a navegação em curso presa num portão do dublê.

use std::sync::Arc;

use gpui::{
    AppContext, Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    TestAppContext, VisualTestContext, point, px,
};
use hog_audio::transitions::PlayerState;
use hog_player_gpui::store::Store;
use hog_player_gpui::theme::{CLOCK_HEIGHT, GAP, TITLE_BLOCK, TRANSPORT_HEIGHT};
use hog_player_gpui::views::root::RootView;
use hog_player_testing::{FakePlayer, Gate, PRAZO, config_de_teste, itens};

/// Cinco faixas, a primeira carregada: Próxima fica habilitada durante as três navegações.
fn janela(
    cx: &mut TestAppContext,
    estado: PlayerState,
) -> (Arc<FakePlayer>, Entity<Store>, &mut VisualTestContext) {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&[
            "/tmp/a.flac",
            "/tmp/b.flac",
            "/tmp/c.flac",
            "/tmp/d.flac",
            "/tmp/e.flac",
        ]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = estado;
    });
    let player = fake.clone();
    let store = cx.new(move |cx| Store::new(player, config_de_teste(), cx));
    store.update(cx, |s, _| {
        assert!(s.wait_idle(PRAZO) && s.wait_entries(PRAZO))
    });
    store.update(cx, |s, cx| s.tick(cx));
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));
    window.run_until_parked();
    (fake, store, window)
}

/// O centro do botão Próxima, pela coluna: capa, título, relógio e o transporte, com os três
/// botões (40, 56, 40, separados por 24) centrados na largura da capa.
fn proxima(window: &mut VisualTestContext) -> Point<Pixels> {
    const BUTTONS_WIDTH: f32 = 40.0 + 24.0 + 56.0 + 24.0 + 40.0;
    let coluna = window.debug_bounds("coluna").expect("coluna");
    let capa = f32::from(coluna.size.width);
    let topo_do_transporte = capa + GAP + TITLE_BLOCK + GAP + CLOCK_HEIGHT + GAP;
    point(
        coluna.origin.x + px(capa / 2.0 + BUTTONS_WIDTH / 2.0 - 20.0),
        coluna.origin.y + px(topo_do_transporte + TRANSPORT_HEIGHT / 2.0),
    )
}

/// O clique que o macOS entrega: o `clickCount` sobe com cliques seguidos no mesmo ponto.
fn clicar(window: &mut VisualTestContext, onde: Point<Pixels>, click_count: usize) {
    window.simulate_event(MouseDownEvent {
        position: onde,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
        first_mouse: false,
    });
    window.simulate_event(MouseUpEvent {
        position: onde,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
    });
}

fn navegacoes(fake: &FakePlayer) -> Vec<i32> {
    fake.with(|s| s.navegacoes.clone())
}

/// O tique do poll e um quadro, como a main faria enquanto o comando corre.
fn tique_e_quadro(window: &mut VisualTestContext, store: &Entity<Store>) {
    store.update(window, |s, cx| s.tick(cx));
    window.run_until_parked();
}

#[gpui::test]
fn um_clique_em_proxima_navega_um_passo(cx: &mut TestAppContext) {
    let (fake, store, window) = janela(cx, PlayerState::Loaded);
    let onde = proxima(window);

    clicar(window, onde, 1);
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));

    assert_eq!(navegacoes(&fake), [1]);
}

/// O sintoma do app real: três cliques com a primeira navegação ainda em curso. A intenção do
/// controlador é a primeira carregar um passo e os dois cliques seguintes somarem numa só.
#[gpui::test]
fn tres_cliques_com_a_navegacao_presa_viram_um_mais_dois(cx: &mut TestAppContext) {
    let (fake, store, window) = janela(cx, PlayerState::Playing);
    let onde = proxima(window);
    let portao = Gate::new();
    fake.with(|s| s.portao_da_navegacao = Some(portao.clone()));

    clicar(window, onde, 1);
    assert!(fake.esperar(PRAZO, |s| s.navegacoes.len() == 1));
    tique_e_quadro(window, &store);
    clicar(window, onde, 2);
    tique_e_quadro(window, &store);
    clicar(window, onde, 3);
    portao.open();
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));

    assert_eq!(navegacoes(&fake), [1, 2]);
    assert_eq!(fake.with(|s| s.indice_atual), Some(3));
}

/// A mesma coisa sem tique nem quadro entre os cliques e com `clickCount` 1 em todos: separa o
/// caminho do evento do resto.
#[gpui::test]
fn tres_cliques_simples_seguidos_com_a_navegacao_presa_viram_um_mais_dois(cx: &mut TestAppContext) {
    let (fake, store, window) = janela(cx, PlayerState::Loaded);
    let onde = proxima(window);
    let portao = Gate::new();
    fake.with(|s| s.portao_da_navegacao = Some(portao.clone()));

    clicar(window, onde, 1);
    assert!(fake.esperar(PRAZO, |s| s.navegacoes.len() == 1));
    clicar(window, onde, 1);
    clicar(window, onde, 1);
    portao.open();
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));

    assert_eq!(navegacoes(&fake), [1, 2]);
    assert_eq!(fake.with(|s| s.indice_atual), Some(3));
}

/// Um clique durante uma navegação já em curso abre outra navegação e não soma nela: a thread
/// tira o pendente ao começar, e só os cliques que chegam enquanto a navegação espera a vez
/// somam.
#[gpui::test]
fn cliques_espacados_alem_do_comando_viram_tres_navegacoes_de_um(cx: &mut TestAppContext) {
    let (fake, store, window) = janela(cx, PlayerState::Playing);
    let onde = proxima(window);

    clicar(window, onde, 1);
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));
    let portao = Gate::new();
    fake.with(|s| s.portao_da_navegacao = Some(portao.clone()));
    clicar(window, onde, 1);
    assert!(fake.esperar(PRAZO, |s| s.navegacoes.len() == 2));
    clicar(window, onde, 1);
    portao.open();
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));

    assert_eq!(navegacoes(&fake), [1, 1, 1]);
}
