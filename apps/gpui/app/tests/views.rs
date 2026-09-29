use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyView, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    Styled, TestAppContext, Window, div, px,
};
use hog_audio::transitions::PlayerState;
use hog_player_gpui::actions;
use hog_player_gpui::store::{Signal, Store, wake_channel};
use hog_player_gpui::theme::{
    CLOCK_HEIGHT, STATUS_GAP, STATUS_HEIGHT, TECHNICAL_HEIGHT, TITLE_BLOCK,
};
use hog_player_gpui::views::clock::ClockView;
use hog_player_gpui::views::cover::CoverView;
use hog_player_gpui::views::root::RootView;
use hog_player_gpui::views::status::StatusView;
use hog_player_gpui::views::technical::TechnicalView;
use hog_player_gpui::views::track_info::TrackInfoView;
use hog_player_gpui::views::transport::TransportView;
use hog_player_model::controller::Config;
use hog_player_model::entry::QueueEntryInfo;
use hog_player_model::metadata::TrackMetadata;
use hog_player_testing::{FakePlayer, PRAZO, config_de_teste, entrada, itens, pasta_com};

#[gpui::test]
fn o_test_support_do_gpui_funciona_fora_do_zed(cx: &mut TestAppContext) {
    let contador = cx.new(|_| 0_u32);
    contador.update(cx, |n, _| *n += 1);
    assert_eq!(contador.read_with(cx, |n, _| *n), 1);
}

fn carregada(estado: PlayerState) -> Arc<FakePlayer> {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = estado;
        s.decorrido = 1.0;
    });
    fake
}

fn store(cx: &mut TestAppContext, fake: &Arc<FakePlayer>) -> Entity<Store> {
    store_com(cx, fake, config_de_teste())
}

fn store_com(cx: &mut TestAppContext, fake: &Arc<FakePlayer>, config: Config) -> Entity<Store> {
    let player = fake.clone();
    cx.new(move |cx| Store::new(player, config, cx))
}

/// Quantas vezes cada sinal do `Store` notificou.
#[derive(Default)]
struct Notificacoes {
    tela: Cell<u32>,
    relogio: Cell<u32>,
    fila: Cell<u32>,
    linhas: Cell<u32>,
    volume: Cell<u32>,
    erro: Cell<u32>,
}

type Campo = fn(&Notificacoes) -> &Cell<u32>;

fn contar_sinais(cx: &mut TestAppContext, store: &Entity<Store>) -> Rc<Notificacoes> {
    let contas = Rc::new(Notificacoes::default());
    cx.update(|cx| {
        let s = store.read(cx).signals.clone();
        let pares: [(&Entity<Signal>, Campo); 6] = [
            (&s.display, |n| &n.tela),
            (&s.clock, |n| &n.relogio),
            (&s.queue, |n| &n.fila),
            (&s.entries, |n| &n.linhas),
            (&s.volume, |n| &n.volume),
            (&s.error, |n| &n.erro),
        ];
        for (sinal, campo) in pares {
            let contas = contas.clone();
            cx.observe(sinal, move |_, _| {
                let conta = campo(&contas);
                conta.set(conta.get() + 1);
            })
            .detach();
        }
    });
    contas
}

/// Tiques pelo `Store`, que publica os sinais, até `pronto` valer, com teto. O `wait_*` do
/// controlador não serve aqui: ele tica por baixo do `Store` e a mudança não vira sinal.
fn tique_ate(
    cx: &mut TestAppContext,
    store: &Entity<Store>,
    pronto: impl Fn(&Store) -> bool,
) -> bool {
    let limite = Instant::now() + PRAZO;
    loop {
        let feito = store.update(cx, |s, cx| {
            s.tick(cx);
            pronto(s)
        });
        if feito {
            return true;
        }
        if Instant::now() >= limite {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Uma carregada com as linhas já lidas e o `Store` já tendo seguido a faixa atual: o próximo
/// tique só publica o que o teste mudar.
fn assentada(cx: &mut TestAppContext, estado: PlayerState) -> (Arc<FakePlayer>, Entity<Store>) {
    let fake = carregada(estado);
    let store = store(cx, &fake);
    store.update(cx, |s, _| {
        assert!(s.wait_idle(PRAZO) && s.wait_entries(PRAZO))
    });
    store.update(cx, |s, cx| s.tick(cx));
    (fake, store)
}

/// Um tique que só avança o relógio notifica só o sinal do relógio: é o que deixa as outras
/// views em cache.
#[gpui::test]
fn o_tique_do_relogio_notifica_so_o_sinal_do_relogio(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Playing);
    let store = store(cx, &fake);
    // As linhas carregando depois mudariam o título mostrado e sujariam a contagem.
    store.update(cx, |s, _| {
        assert!(s.wait_idle(PRAZO) && s.wait_entries(PRAZO))
    });
    store.update(cx, |s, cx| s.tick(cx));

    let relogio = Rc::new(Cell::new(0));
    let tela = Rc::new(Cell::new(0));
    let fila = Rc::new(Cell::new(0));
    cx.update(|cx| {
        let signals = store.read(cx).signals.clone();
        for (sinal, conta) in [
            (&signals.clock, &relogio),
            (&signals.display, &tela),
            (&signals.queue, &fila),
        ] {
            let conta = conta.clone();
            cx.observe(sinal, move |_, _| conta.set(conta.get() + 1))
                .detach();
        }
    });

    fake.with(|s| s.decorrido = 2.0);
    store.update(cx, |s, cx| s.tick(cx));

    assert_eq!((relogio.get(), tela.get(), fila.get()), (1, 0, 0));
}

#[gpui::test]
fn mudanca_de_estado_notifica_a_tela(cx: &mut TestAppContext) {
    let (fake, store) = assentada(cx, PlayerState::Loaded);
    let n = contar_sinais(cx, &store);

    fake.with(|s| s.estado = PlayerState::Playing);
    store.update(cx, |s, cx| s.tick(cx));

    assert_eq!(n.tela.get(), 1);
}

/// Sem a faixa atual mudar: a notificação vem do `Changes::queue`, não da seleção que segue a
/// faixa atual.
#[gpui::test]
fn faixa_nova_na_fila_notifica_a_fila(cx: &mut TestAppContext) {
    let (fake, store) = assentada(cx, PlayerState::Loaded);
    let n = contar_sinais(cx, &store);

    fake.acrescentar("/tmp/d.flac");
    fake.with(|s| s.versao_fila += 1);
    store.update(cx, |s, cx| s.tick(cx));

    assert_eq!(n.fila.get(), 1);
}

#[gpui::test]
fn linha_carregada_notifica_as_linhas(cx: &mut TestAppContext) {
    let (fake, store) = assentada(cx, PlayerState::Loaded);
    let n = contar_sinais(cx, &store);

    fake.acrescentar("/tmp/d.flac");
    fake.with(|s| s.versao_fila += 1);
    let carregou = tique_ate(cx, &store, |s| {
        s.model().entries.contains_key("/tmp/d.flac")
    });

    assert!(carregou);
    assert!(
        n.linhas.get() >= 1,
        "linhas notificou {} vez(es)",
        n.linhas.get()
    );
}

/// O volume mudado por fora (teclado do Mac, outro app) com nada nosso mexendo nele.
#[gpui::test]
fn volume_mudado_no_device_notifica_o_volume(cx: &mut TestAppContext) {
    let (fake, store) = assentada(cx, PlayerState::Playing);
    let n = contar_sinais(cx, &store);

    fake.with(|s| s.volume_real = Some(0.8));
    store.update(cx, |s, cx| s.tick(cx));

    assert_eq!(n.volume.get(), 1);
}

#[gpui::test]
fn play_recusado_notifica_o_erro(cx: &mut TestAppContext) {
    let (fake, store) = assentada(cx, PlayerState::Loaded);
    fake.with(|s| s.falha_ao_tocar = Some("o device recusou".to_string()));
    let n = contar_sinais(cx, &store);

    store.update(cx, |s, _| s.toggle());
    let apareceu = tique_ate(cx, &store, |s| s.model().error.is_some());

    assert!(apareceu);
    assert_eq!(n.erro.get(), 1);
}

/// Conta as notificações de uma view na posição `i` de `contas`.
fn observar<V: 'static>(
    cx: &mut TestAppContext,
    view: &Entity<V>,
    contas: &Rc<Vec<Cell<u32>>>,
    i: usize,
) {
    let contas = contas.clone();
    cx.update(|cx| {
        cx.observe(view, move |_, _| contas[i].set(contas[i].get() + 1))
            .detach()
    });
}

/// Cada view redesenha só com o sinal da parte do modelo que ela lê.
#[gpui::test]
fn cada_view_observa_so_o_seu_sinal(cx: &mut TestAppContext) {
    const NOMES: [&str; 7] = [
        "capa",
        "faixa",
        "relogio",
        "transporte",
        "tecnica",
        "aviso",
        "raiz",
    ];
    let fake = FakePlayer::new();
    let store = store(cx, &fake);
    let contas: Rc<Vec<Cell<u32>>> = Rc::new(NOMES.iter().map(|_| Cell::new(0)).collect());
    let (raiz, _) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));
    let capa = cx.new(|cx| CoverView::new(store.clone(), cx));
    let faixa = cx.new(|cx| TrackInfoView::new(store.clone(), cx));
    let relogio = cx.new(|cx| ClockView::new(store.clone(), cx));
    let transporte = cx.new(|cx| TransportView::new(store.clone(), cx));
    let tecnica = cx.new(|cx| TechnicalView::new(store.clone(), cx));
    let aviso = cx.new(|cx| StatusView::new(store.clone(), cx));
    observar(cx, &capa, &contas, 0);
    observar(cx, &faixa, &contas, 1);
    observar(cx, &relogio, &contas, 2);
    observar(cx, &transporte, &contas, 3);
    observar(cx, &tecnica, &contas, 4);
    observar(cx, &aviso, &contas, 5);
    observar(cx, &raiz, &contas, 6);

    let signals = cx.update(|cx| store.read(cx).signals.clone());
    let mut visto = Vec::new();
    for (nome, sinal) in [
        ("display", &signals.display),
        ("clock", &signals.clock),
        ("queue", &signals.queue),
        ("entries", &signals.entries),
        ("volume", &signals.volume),
        ("error", &signals.error),
    ] {
        for conta in contas.iter() {
            conta.set(0);
        }
        cx.update(|cx| sinal.update(cx, |_, cx| cx.notify()));
        let quem: Vec<&str> = NOMES
            .iter()
            .zip(contas.iter())
            .filter(|(_, conta)| conta.get() > 0)
            .map(|(view, _)| *view)
            .collect();
        visto.push((nome, quem));
    }

    assert_eq!(
        visto,
        vec![
            ("display", vec!["capa", "faixa", "transporte", "tecnica"]),
            ("clock", vec!["relogio"]),
            ("queue", vec![]),
            ("entries", vec![]),
            ("volume", vec![]),
            ("error", vec!["aviso", "raiz"]),
        ]
    );
}

/// Ninguém chama `tick()` aqui e o relógio virtual do teste não anda, então o poll de 100 ms
/// não dispara: se o modelo mostra o play, foi o wake do controlador que levou ao tique.
#[gpui::test]
fn um_wake_do_controlador_leva_a_um_tique(cx: &mut TestAppContext) {
    // O wake vem da thread de comandos do controlador; sem isto o escalonador de teste do GPUI
    // recusa uma task acordada fora da thread dele.
    cx.executor().allow_parking();
    let fake = carregada(PlayerState::Loaded);
    let (wake, woken) = wake_channel();
    let store = store_com(
        cx,
        &fake,
        Config {
            wake,
            ..config_de_teste()
        },
    );
    store.update(cx, |s, cx| s.start(woken, cx));

    store.update(cx, |s, _| s.toggle());
    let limite = Instant::now() + PRAZO;
    let tocando = loop {
        cx.run_until_parked();
        let tocando = store.read_with(cx, |s, _| s.model().display.is_playing);
        if tocando || Instant::now() >= limite {
            break tocando;
        }
        std::thread::sleep(Duration::from_millis(1));
    };

    assert!(fake.chamou("play"));
    assert!(tocando);
}

/// Sem wake nenhum (o do `config_de_teste` não faz nada), só o poll tica.
#[gpui::test]
fn o_poll_tica_a_cada_100_ms(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Playing);
    let store = store(cx, &fake);
    let (_, woken) = wake_channel();
    store.update(cx, |s, cx| s.start(woken, cx));
    // O timer do poll é armado quando a task roda pela primeira vez, no instante zero.
    cx.run_until_parked();
    fake.with(|s| s.decorrido = 2.0);
    let decorrido =
        |cx: &mut TestAppContext| store.read_with(cx, |s, _| s.model().clock.elapsed.clone());

    cx.executor().advance_clock(Duration::from_millis(99));
    cx.run_until_parked();
    let antes = decorrido(cx);
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();

    assert_eq!((antes.as_str(), decorrido(cx).as_str()), ("0:01", "0:02"));
}

#[gpui::test]
fn espaco_toca(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Loaded);
    let store = store(cx, &fake);
    cx.update(|cx| {
        actions::bind(cx);
        actions::register(&store, cx);
    });
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));

    window.simulate_keystrokes("space");
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));

    assert!(fake.chamou("play"));
}

#[gpui::test]
fn cmd_o_abre_o_painel_e_acrescenta_o_que_foi_escolhido(cx: &mut TestAppContext) {
    let pasta = pasta_com("cmd-o", &["a.flac"]);
    let fake = FakePlayer::new();
    let store = store(cx, &fake);
    cx.update(|cx| {
        actions::bind(cx);
        actions::register(&store, cx);
    });
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));

    window.simulate_keystrokes("cmd-o");
    assert!(cx.did_prompt_for_paths());
    let escolhida = pasta.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![escolhida]));
    cx.run_until_parked();
    store.update(cx, |s, _| assert!(s.wait_idle(PRAZO)));

    assert_eq!(fake.with(|s| s.itens.len()), 1);
    std::fs::remove_dir_all(pasta).unwrap();
}

/// Sem cache e sem altura imposta, cada view ocupa o que o conteúdo dela pede. É essa altura
/// que tem de caber no slot fixo que a raiz dá à view em cache; o que passar é cortado.
struct AlturaNatural {
    largura: f32,
    views: Vec<(&'static str, AnyView)>,
}

impl Render for AlturaNatural {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .children(self.views.iter().map(|(nome, view)| {
                let nome = *nome;
                div()
                    .debug_selector(move || nome.to_string())
                    .w(px(self.largura))
                    .child(view.clone())
            }))
    }
}

/// A faixa da verificação visual, com artista e álbum, igual nas linhas e nos metadados.
fn config_com_tags() -> Config {
    Config {
        load_entry: Arc::new(|_: &str| QueueEntryInfo {
            artist: Some("Coldplay".to_string()),
            album: Some("Live 2012".to_string()),
            ..entrada("Hurts Like Heaven (Live)")
        }),
        load_metadata: Arc::new(|_: &str| TrackMetadata {
            title: "Hurts Like Heaven (Live)".to_string(),
            artist: Some("Coldplay".to_string()),
            album: Some("Live 2012".to_string()),
            artwork: None,
        }),
        ..config_de_teste()
    }
}

#[gpui::test]
fn cada_texto_da_coluna_cabe_no_seu_slot(cx: &mut TestAppContext) {
    const LARGURA: f32 = 320.0;
    let fake = carregada(PlayerState::Loaded);
    let store = store_com(cx, &fake, config_com_tags());
    // O select traz o formato, e com ele a linha técnica.
    store.update(cx, |s, cx| s.play_row(1, cx));
    store.update(cx, |s, _| {
        assert!(s.wait_idle(PRAZO) && s.wait_entries(PRAZO))
    });
    fake.with(|s| {
        s.problema = Some("o device de saída foi desconectado; a reprodução parou e o hog foi solto, então escolha outro device".repeat(2));
        s.seq_problema += 1;
    });
    store.update(cx, |s, cx| s.tick(cx));
    let model = store.read_with(cx, |s, _| s.model().clone());
    assert!(!model.display.artist.is_empty() && !model.display.album.is_empty());
    assert!(!model.display.technical_line.is_empty());
    assert!(model.error.is_some());

    let (_, window) = cx.add_window_view(|_, cx| AlturaNatural {
        largura: LARGURA,
        views: vec![
            (
                "faixa",
                cx.new(|cx| TrackInfoView::new(store.clone(), cx)).into(),
            ),
            (
                "relogio",
                cx.new(|cx| ClockView::new(store.clone(), cx)).into(),
            ),
            (
                "tecnica",
                cx.new(|cx| TechnicalView::new(store.clone(), cx)).into(),
            ),
            (
                "aviso",
                cx.new(|cx| StatusView::new(store.clone(), cx)).into(),
            ),
        ],
    });
    window.run_until_parked();

    let medido: Vec<(&str, f32, f32)> = [
        ("faixa", TITLE_BLOCK),
        ("relogio", CLOCK_HEIGHT),
        ("tecnica", TECHNICAL_HEIGHT),
        ("aviso", STATUS_HEIGHT),
    ]
    .into_iter()
    .map(|(nome, slot)| {
        let altura = window.debug_bounds(nome).expect(nome).size.height;
        (nome, f32::from(altura), slot)
    })
    .collect();
    for (nome, altura, _) in &medido {
        assert!(*altura > 0.0, "{nome} sem altura: {medido:?}");
    }
    let estouram: Vec<_> = medido
        .iter()
        .filter(|(_, altura, slot)| altura > slot)
        .collect();
    assert!(
        estouram.is_empty(),
        "passam do slot: {estouram:?}; todas (nome, altura, slot): {medido:?}"
    );
}

/// Sem aviso, a fila começa na altura da capa, como no SwiftUI; com aviso, desce a altura dele
/// mais o espaço.
#[gpui::test]
fn a_fila_so_desce_quando_ha_aviso(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Loaded);
    let store = store(cx, &fake);
    store.update(cx, |s, _| assert!(s.wait_idle(PRAZO)));
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));
    window.run_until_parked();
    let topo = |window: &mut gpui::VisualTestContext, nome: &'static str| {
        f32::from(window.debug_bounds(nome).expect(nome).origin.y)
    };
    let sem_aviso = topo(window, "fila") - topo(window, "coluna");

    fake.with(|s| {
        s.problema = Some("o device sumiu".to_string());
        s.seq_problema += 1;
    });
    store.update(window, |s, cx| s.tick(cx));
    window.run_until_parked();
    let com_aviso = topo(window, "fila") - topo(window, "coluna");

    assert_eq!((sem_aviso, com_aviso), (0.0, STATUS_HEIGHT + STATUS_GAP));
}
