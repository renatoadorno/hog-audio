use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyView, AppContext, Bounds, Context, Entity, ExternalPaths, FileDropEvent, InteractiveElement,
    IntoElement, Modifiers, MouseButton, ParentElement, Pixels, Point, Render, StyleRefinement,
    Styled, TestAppContext, VisualTestContext, Window, div, point, px, size,
};
use hog_audio::transitions::PlayerState;
use hog_player_gpui::actions;
use hog_player_gpui::store::{Signal, Store, wake_channel};
use hog_player_gpui::theme::{
    CLOCK_HEIGHT, STATUS_GAP, STATUS_HEIGHT, TECHNICAL_HEIGHT, TITLE_BLOCK, VOLUME_HEIGHT,
};
use hog_player_gpui::views::backdrop::BackdropView;
use hog_player_gpui::views::clock::ClockView;
use hog_player_gpui::views::cover::CoverView;
use hog_player_gpui::views::queue::QueueView;
use hog_player_gpui::views::root::RootView;
use hog_player_gpui::views::status::StatusView;
use hog_player_gpui::views::technical::TechnicalView;
use hog_player_gpui::views::track_info::TrackInfoView;
use hog_player_gpui::views::transport::TransportView;
use hog_player_gpui::views::volume::{VolumeView, percent};
use hog_player_model::controller::Config;
use hog_player_model::entry::QueueEntryInfo;
use hog_player_model::metadata::{TrackMetadata, load_track_metadata};
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
    const NOMES: [&str; 10] = [
        "capa",
        "fundo",
        "faixa",
        "relogio",
        "transporte",
        "tecnica",
        "aviso",
        "volume",
        "fila",
        "raiz",
    ];
    let fake = FakePlayer::new();
    let store = store(cx, &fake);
    let contas: Rc<Vec<Cell<u32>>> = Rc::new(NOMES.iter().map(|_| Cell::new(0)).collect());
    let (raiz, janela) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));
    let volume =
        janela.update(|window, cx| cx.new(|cx| VolumeView::new(store.clone(), window, cx)));
    let capa = cx.new(|cx| CoverView::new(store.clone(), cx));
    let fundo = cx.new(|cx| BackdropView::new(store.clone(), cx));
    let faixa = cx.new(|cx| TrackInfoView::new(store.clone(), cx));
    let relogio = cx.new(|cx| ClockView::new(store.clone(), cx));
    let transporte = cx.new(|cx| TransportView::new(store.clone(), cx));
    let tecnica = cx.new(|cx| TechnicalView::new(store.clone(), cx));
    let aviso = cx.new(|cx| StatusView::new(store.clone(), cx));
    let fila = cx.new(|cx| QueueView::new(store.clone(), cx));
    observar(cx, &capa, &contas, 0);
    observar(cx, &fundo, &contas, 1);
    observar(cx, &faixa, &contas, 2);
    observar(cx, &relogio, &contas, 3);
    observar(cx, &transporte, &contas, 4);
    observar(cx, &tecnica, &contas, 5);
    observar(cx, &aviso, &contas, 6);
    observar(cx, &volume, &contas, 7);
    observar(cx, &fila, &contas, 8);
    observar(cx, &raiz, &contas, 9);

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
            (
                "display",
                vec!["capa", "fundo", "faixa", "transporte", "tecnica"]
            ),
            ("clock", vec!["relogio"]),
            ("queue", vec!["fila"]),
            ("entries", vec!["fila"]),
            ("volume", vec!["volume"]),
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

#[gpui::test]
fn soltar_arquivos_na_janela_acrescenta_a_fila(cx: &mut TestAppContext) {
    let pasta = pasta_com("soltar-janela", &["a.flac", "b.flac"]);
    let fake = FakePlayer::new();
    let store = store(cx, &fake);
    let (root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));

    root.update(window, |root, cx| root.accept_drop(vec![pasta.clone()], cx));
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));

    assert_eq!(fake.with(|s| s.itens.len()), 2);
    std::fs::remove_dir_all(pasta).unwrap();
}

/// O arrasto do Finder pela fiação do GPUI: o `Entered` abre o drag com os caminhos e o `Submit`
/// vira o mouse-up que chega ao `on_drop` da raiz.
#[gpui::test]
fn soltar_do_finder_sobre_a_janela_acrescenta_a_fila(cx: &mut TestAppContext) {
    let pasta = pasta_com("soltar-finder", &["a.flac", "b.flac"]);
    let fake = FakePlayer::new();
    let store = store(cx, &fake);
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));
    window.run_until_parked();
    let onde = window
        .debug_bounds("fila-vazia")
        .expect("fila-vazia")
        .center();

    window.simulate_event(FileDropEvent::Entered {
        position: onde,
        paths: ExternalPaths([pasta.clone()].into_iter().collect()),
    });
    window.simulate_event(FileDropEvent::Submit { position: onde });
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));

    assert_eq!(fake.with(|s| s.itens.len()), 2);
    assert!(!window.read(|cx| cx.has_active_drag()));
    std::fs::remove_dir_all(pasta).unwrap();
}

/// Caminho de uma fixture de `testdata/` na raiz do repositório. Reprova se ela não existir.
fn fixture(nome: &str) -> String {
    let caminho = format!("{}/../../../testdata/{nome}", env!("CARGO_MANIFEST_DIR"));
    assert!(
        std::path::Path::new(&caminho).exists(),
        "fixture ausente: {caminho}\nrode `make fixtures` na raiz do repositório"
    );
    caminho
}

/// Quais dos dois ramos da capa o último quadro desenhou: (imagem, placeholder).
fn ramos_da_capa(window: &mut VisualTestContext) -> (bool, bool) {
    (
        window.debug_bounds("capa-imagem").is_some(),
        window.debug_bounds("capa-vazia").is_some(),
    )
}

/// A arte vem do leitor de verdade, numa faixa com capa: é o caminho do app.
#[gpui::test]
fn a_capa_mostra_a_imagem_com_arte_e_o_placeholder_sem(cx: &mut TestAppContext) {
    let faixa = fixture("meta_fixture.flac");
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&[&faixa]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Loaded;
    });
    let store = store_com(
        cx,
        &fake,
        Config {
            load_metadata: Arc::new(load_track_metadata),
            ..config_de_teste()
        },
    );
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));
    window.run_until_parked();
    let sem_arte = ramos_da_capa(window);

    store.update(window, |s, cx| s.play_row(1, cx));
    let chegou = tique_ate(window, &store, |s| s.model().display.artwork.is_some());
    window.run_until_parked();
    let com_arte = ramos_da_capa(window);

    assert!(chegou, "a arte não chegou ao modelo");
    assert_eq!((sem_arte, com_arte), ((false, true), (true, false)));
    let coluna = window.debug_bounds("coluna").expect("coluna");
    let imagem = window.debug_bounds("capa-imagem").expect("capa-imagem");
    assert!(
        imagem.size.width >= coluna.size.width - px(2.0),
        "a capa não ocupa a coluna: {imagem:?} em {coluna:?}"
    );
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

    let (_, window) = cx.add_window_view(|window, cx| AlturaNatural {
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
            (
                "volume",
                cx.new(|cx| VolumeView::new(store.clone(), window, cx))
                    .into(),
            ),
        ],
    });
    window.run_until_parked();

    let medido: Vec<(&str, f32, f32)> = [
        ("faixa", TITLE_BLOCK),
        ("relogio", CLOCK_HEIGHT),
        ("tecnica", TECHNICAL_HEIGHT),
        ("aviso", STATUS_HEIGHT),
        ("volume", VOLUME_HEIGHT),
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

#[gpui::test]
fn o_slider_publica_na_hora_e_notifica_so_o_volume(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Playing);
    let store = store(cx, &fake);
    store.update(cx, |s, _| {
        assert!(s.wait_idle(PRAZO) && s.wait_entries(PRAZO))
    });
    store.update(cx, |s, cx| s.tick(cx));
    let volume = Rc::new(Cell::new(0));
    let tela = Rc::new(Cell::new(0));
    cx.update(|cx| {
        let signals = store.read(cx).signals.clone();
        for (sinal, conta) in [(&signals.volume, &volume), (&signals.display, &tela)] {
            let conta = conta.clone();
            cx.observe(sinal, move |_, _| conta.set(conta.get() + 1))
                .detach();
        }
    });

    store.update(cx, |s, cx| s.apply_volume(0.3, cx));

    assert_eq!(store.read_with(cx, |s, _| s.model().volume.value), 0.3);
    assert_eq!((volume.get(), tela.get()), (1, 0));
    store.update(cx, |s, _| assert!(s.wait_idle(PRAZO)));
    assert_eq!(fake.with(|s| s.volume_aplicado), Some(0.3));
}

#[gpui::test]
fn o_passo_do_volume_nao_passa_dos_limites(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Playing);
    let store = store(cx, &fake);

    store.update(cx, |s, cx| {
        s.apply_volume(0.98, cx);
        s.nudge_volume(0.05, cx);
    });
    assert_eq!(store.read_with(cx, |s, _| s.model().volume.value), 1.0);

    store.update(cx, |s, cx| s.nudge_volume(-2.0, cx));
    assert_eq!(store.read_with(cx, |s, _| s.model().volume.value), 0.0);
    store.update(cx, |s, _| assert!(s.wait_idle(PRAZO)));
}

/// O texto da tela e o nó do slider usam o mesmo número: truncado, então 0,29 (que vira
/// 28,999…) dá 28 nos dois.
#[test]
fn o_percentual_do_volume_trunca_como_o_texto_da_tela() {
    assert_eq!(percent(0.55), 55);
    assert_eq!(percent(0.29), 28);
}

/// O trilho útil, medido no layout: o trilho menos o raio do botão em cada ponta. É o caminho do
/// centro do botão, de 0 a 1. Só existe no quadro em que a `VolumeView` redesenhou: o prepaint
/// reaproveitado de uma view em cache não repõe o `debug_bounds`.
fn trilho_util(window: &mut VisualTestContext) -> Bounds<Pixels> {
    let trilho = window.debug_bounds("trilho-volume").expect("trilho-volume");
    let raio = window
        .debug_bounds("botao-volume")
        .expect("botao-volume")
        .size
        .width
        / 2.0;
    let util = Bounds {
        origin: point(trilho.left() + raio, trilho.top()),
        size: size(trilho.size.width - raio * 2.0, trilho.size.height),
    };
    assert!(
        f32::from(util.size.width) > 0.0,
        "trilho sem largura: {trilho:?}"
    );
    util
}

/// A raiz inteira numa janela de teste, tocando, e o trilho útil do volume.
fn volume_na_janela(
    cx: &mut TestAppContext,
) -> (
    Arc<FakePlayer>,
    Entity<Store>,
    &mut VisualTestContext,
    Bounds<Pixels>,
) {
    let (fake, store) = assentada(cx, PlayerState::Playing);
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));
    window.run_until_parked();
    let trilho = trilho_util(window);
    (fake, store, window, trilho)
}

fn no_trilho(trilho: Bounds<Pixels>, fracao: f32) -> Point<Pixels> {
    point(
        trilho.left() + trilho.size.width * fracao,
        trilho.center().y,
    )
}

fn apertar(window: &mut VisualTestContext, onde: Point<Pixels>) {
    window.simulate_mouse_down(onde, MouseButton::Left, Modifiers::none());
}

/// Com o botão apertado.
fn arrastar(window: &mut VisualTestContext, onde: Point<Pixels>) {
    window.simulate_mouse_move(onde, MouseButton::Left, Modifiers::none());
}

fn soltar(window: &mut VisualTestContext, onde: Point<Pixels>) {
    window.simulate_mouse_up(onde, MouseButton::Left, Modifiers::none());
}

fn volume(window: &mut VisualTestContext, store: &Entity<Store>) -> f64 {
    store.read_with(window, |s, _| s.model().volume.value)
}

/// Três casas bastam: o valor sai de uma divisão em `f32` sobre a posição em pixels.
fn casas(valor: f64) -> f64 {
    (valor * 1000.0).round() / 1000.0
}

/// O `editing` não aparece no modelo; aparece no controlador ignorando o volume mudado por fora
/// enquanto a mão está no slider (`follow_device_volume`). Devolve se o slider seguiu o device.
fn segue_o_device(
    window: &mut VisualTestContext,
    fake: &FakePlayer,
    store: &Entity<Store>,
) -> bool {
    const DE_FORA: f32 = 0.125;
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));
    fake.with(|s| s.volume_real = Some(DE_FORA));
    store.update(window, |s, cx| s.tick(cx));
    volume(window, store) == f64::from(DE_FORA)
}

#[gpui::test]
fn apertar_no_trilho_leva_o_volume_ao_ponto_e_liga_a_edicao(cx: &mut TestAppContext) {
    let (fake, store, window, trilho) = volume_na_janela(cx);

    apertar(window, no_trilho(trilho, 0.3));

    assert_eq!(casas(volume(window, &store)), 0.3);
    assert!(
        !segue_o_device(window, &fake, &store),
        "o volume de fora moveu o slider com a mão nele"
    );
}

#[gpui::test]
fn arrastar_faz_o_volume_acompanhar_e_notifica_so_o_volume(cx: &mut TestAppContext) {
    let (_fake, store, window, trilho) = volume_na_janela(cx);
    apertar(window, no_trilho(trilho, 0.2));
    let n = contar_sinais(window, &store);

    let mut vistos = Vec::new();
    for fracao in [0.6, 0.8, 0.4] {
        arrastar(window, no_trilho(trilho, fracao));
        vistos.push(casas(volume(window, &store)));
    }

    assert_eq!(vistos, vec![0.6, 0.8, 0.4]);
    assert_eq!((n.volume.get(), n.tela.get()), (3, 0));
}

#[gpui::test]
fn soltar_o_botao_desliga_a_edicao(cx: &mut TestAppContext) {
    let (fake, store, window, trilho) = volume_na_janela(cx);
    apertar(window, no_trilho(trilho, 0.3));
    arrastar(window, no_trilho(trilho, 0.6));

    soltar(window, no_trilho(trilho, 0.6));

    assert!(
        segue_o_device(window, &fake, &store),
        "o slider continuou em edição depois de soltar"
    );
}

#[gpui::test]
fn soltar_fora_do_trilho_encerra_o_arrasto(cx: &mut TestAppContext) {
    let (fake, store, window, trilho) = volume_na_janela(cx);
    let fora = point(no_trilho(trilho, 0.7).x, trilho.bottom() + px(100.0));
    assert!(!trilho.contains(&fora));
    apertar(window, no_trilho(trilho, 0.3));
    arrastar(window, fora);
    assert_eq!(casas(volume(window, &store)), 0.7);

    soltar(window, fora);
    window.simulate_mouse_move(no_trilho(trilho, 0.9), None, Modifiers::none());

    assert_eq!(casas(volume(window, &store)), 0.7);
    assert!(
        segue_o_device(window, &fake, &store),
        "o slider continuou em edição depois de soltar fora do trilho"
    );
}

#[gpui::test]
fn arrastar_alem_das_pontas_fica_em_zero_e_em_um(cx: &mut TestAppContext) {
    let (_fake, store, window, trilho) = volume_na_janela(cx);
    let y = trilho.center().y;
    apertar(window, no_trilho(trilho, 0.5));

    arrastar(window, point(trilho.left() - px(50.0), y));
    let esquerda = volume(window, &store);
    arrastar(window, point(trilho.right() + px(50.0), y));
    let direita = volume(window, &store);

    assert_eq!((esquerda, direita), (0.0, 1.0));
}

/// O AppKit pode não entregar o mouse-up (⌘-Tab com o botão apertado). Sem encerrar o arrasto
/// na desativação, o `editing` ficaria preso e o slider pararia de seguir o device.
#[gpui::test]
fn perder_o_foco_no_meio_do_arrasto_encerra_o_arrasto(cx: &mut TestAppContext) {
    let (fake, store, window, trilho) = volume_na_janela(cx);
    // A janela de teste nasce inativa, e desativar uma janela inativa não faz nada.
    window.update(|window, _| window.activate_window());
    window.run_until_parked();
    assert!(window.update(|window, _| window.is_window_active()));
    apertar(window, no_trilho(trilho, 0.3));
    arrastar(window, no_trilho(trilho, 0.6));

    window.deactivate_window();
    arrastar(window, no_trilho(trilho, 0.9));

    assert_eq!(casas(volume(window, &store)), 0.6);
    assert!(
        segue_o_device(window, &fake, &store),
        "o slider continuou em edição depois de perder o foco"
    );
}

#[gpui::test]
fn as_pontas_do_trilho_util_dao_zero_e_um(cx: &mut TestAppContext) {
    let (_fake, store, window, trilho) = volume_na_janela(cx);

    apertar(window, no_trilho(trilho, 0.0));
    soltar(window, no_trilho(trilho, 0.0));
    let esquerda = casas(volume(window, &store));
    apertar(window, no_trilho(trilho, 1.0));
    soltar(window, no_trilho(trilho, 1.0));
    let direita = casas(volume(window, &store));

    assert_eq!((esquerda, direita), (0.0, 1.0));
}

/// Como no NSSlider, o botão fica dentro do trilho: nas pontas, o clique nele cai no trilho e
/// não o desloca.
#[gpui::test]
fn nas_pontas_o_botao_fica_dentro_do_trilho_e_o_clique_nele_nao_o_move(cx: &mut TestAppContext) {
    let (_fake, store, window, _) = volume_na_janela(cx);
    let mut vistos = Vec::new();

    for valor in [0.0, 1.0] {
        store.update(window, |s, cx| s.apply_volume(valor, cx));
        window.run_until_parked();
        let trilho = window.debug_bounds("trilho-volume").expect("trilho-volume");
        let botao = window.debug_bounds("botao-volume").expect("botao-volume");
        let dentro = botao.left() >= trilho.left() && botao.right() <= trilho.right();
        apertar(window, botao.center());
        soltar(window, botao.center());
        vistos.push((dentro, casas(volume(window, &store))));
    }

    assert_eq!(vistos, vec![(true, 0.0), (true, 1.0)]);
}

#[gpui::test]
fn o_primeiro_movimento_ja_move_o_volume(cx: &mut TestAppContext) {
    let (_fake, store, window, trilho) = volume_na_janela(cx);
    apertar(window, no_trilho(trilho, 0.3));

    arrastar(window, no_trilho(trilho, 0.6));

    assert_eq!(casas(volume(window, &store)), 0.6);
}

/// Com um drag do GPUI aberto, cada movimento do mouse redesenha a janela inteira, sem cache.
#[gpui::test]
fn arrastar_o_volume_nao_abre_um_drag_do_gpui(cx: &mut TestAppContext) {
    let (_fake, _store, window, trilho) = volume_na_janela(cx);
    apertar(window, no_trilho(trilho, 0.2));
    arrastar(window, no_trilho(trilho, 0.3));

    arrastar(window, no_trilho(trilho, 0.6));

    assert!(!window.read(|cx| cx.has_active_drag()));
}

/// Com o botão apertado, o macOS reenvia o último movimento a cada 16 ms: a mão parada não pode
/// virar uma publicação e uma escrita no device por quadro. O `wait_idle` entre os movimentos
/// impede o worker de juntar os dois pedidos num só.
#[gpui::test]
fn a_mao_parada_no_arrasto_nao_publica_de_novo(cx: &mut TestAppContext) {
    let (fake, store, window, trilho) = volume_na_janela(cx);
    apertar(window, no_trilho(trilho, 0.3));
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));
    let n = contar_sinais(window, &store);
    let antes = fake.with(|s| s.escritas_de_volume);

    for _ in 0..2 {
        arrastar(window, no_trilho(trilho, 0.6));
        store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));
    }

    let escritas = fake.with(|s| s.escritas_de_volume) - antes;
    assert_eq!((n.volume.get(), escritas), (1, 1));
}

/// O movimento é ouvido na janela inteira: só o arrasto que começou no trilho mexe no volume.
#[gpui::test]
fn arrastar_comecando_fora_do_trilho_nao_mexe_no_volume(cx: &mut TestAppContext) {
    let (_fake, store, window, trilho) = volume_na_janela(cx);
    let fora = point(no_trilho(trilho, 0.3).x, trilho.bottom() + px(100.0));
    apertar(window, fora);

    arrastar(window, no_trilho(trilho, 0.8));

    assert_eq!(volume(window, &store), 0.5);
}

/// O mouse-up pode não chegar (o AppKit não o entrega em todo caso): mover com o botão solto
/// não arrasta.
#[gpui::test]
fn mover_com_o_botao_solto_nao_arrasta(cx: &mut TestAppContext) {
    let (_fake, store, window, trilho) = volume_na_janela(cx);
    apertar(window, no_trilho(trilho, 0.3));

    window.simulate_mouse_move(no_trilho(trilho, 0.8), None, Modifiers::none());

    assert_eq!(casas(volume(window, &store)), 0.3);
}

/// Uma view em cache que só conta quantas vezes redesenhou.
struct ContaRender(Rc<Cell<u32>>);

impl Render for ContaRender {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.set(self.0.get() + 1);
        div().size_full()
    }
}

/// O volume com uma irmã em cache, como na coluna da raiz.
struct VolumeComIrma {
    volume: Entity<VolumeView>,
    irma: Entity<ContaRender>,
}

impl Render for VolumeComIrma {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let slot = |altura: f32| StyleRefinement::default().w(px(320.0)).h(px(altura));
        div()
            .flex()
            .flex_col()
            .child(self.irma.clone().cached(slot(40.0)))
            .child(self.volume.clone().cached(slot(VOLUME_HEIGHT)))
    }
}

#[gpui::test]
fn arrastar_o_volume_nao_redesenha_a_view_irma_em_cache(cx: &mut TestAppContext) {
    let (_fake, store) = assentada(cx, PlayerState::Playing);
    let renders = Rc::new(Cell::new(0));
    let (_, window) = cx.add_window_view(|window, cx| VolumeComIrma {
        volume: cx.new(|cx| VolumeView::new(store.clone(), window, cx)),
        irma: cx.new(|_| ContaRender(renders.clone())),
    });
    window.run_until_parked();
    let trilho = trilho_util(window);
    apertar(window, no_trilho(trilho, 0.2));
    arrastar(window, no_trilho(trilho, 0.3));
    // A troca de modalidade de entrada no primeiro evento do mouse redesenha tudo uma vez.
    renders.set(0);

    for fracao in [0.6, 0.8, 0.4] {
        arrastar(window, no_trilho(trilho, fracao));
    }

    assert_eq!(casas(volume(window, &store)), 0.4);
    assert_eq!(renders.get(), 0);
}

#[gpui::test]
fn clicar_numa_linha_seleciona_e_toca(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Loaded);
    let store = store(cx, &fake);

    store.update(cx, |s, cx| s.play_row(2, cx));
    store.update(cx, |s, _| assert!(s.wait_idle(PRAZO)));

    assert_eq!(store.read_with(cx, |s, _| s.selected()), Some(2));
    assert!(fake.chamou("select"));
    assert_eq!(fake.with(|s| s.indice_atual), Some(1));
}

#[gpui::test]
fn a_selecao_acompanha_a_faixa_atual(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Playing);
    let store = store(cx, &fake);
    store.update(cx, |s, cx| s.tick(cx));
    assert_eq!(store.read_with(cx, |s, _| s.selected()), Some(1));

    fake.with(|s| s.indice_atual = Some(2));
    store.update(cx, |s, cx| s.tick(cx));
    assert_eq!(store.read_with(cx, |s, _| s.selected()), Some(3));
}

#[gpui::test]
fn setas_movem_a_selecao_e_delete_remove_a_selecionada(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Loaded);
    let store = store(cx, &fake);
    cx.update(|cx| {
        actions::bind(cx);
        actions::register(&store, cx);
    });
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));
    store.update(window, |s, cx| s.tick(cx));

    // Em dois passos: de 0, "down down up" com o sentido trocado também termina no id 2
    // (0, 0 pelo limite, 0, 1).
    window.simulate_keystrokes("down down");
    assert_eq!(store.read_with(window, |s, _| s.selected()), Some(3));
    window.simulate_keystrokes("up");
    assert_eq!(store.read_with(window, |s, _| s.selected()), Some(2));

    window.simulate_keystrokes("backspace");
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));

    let ids: Vec<u64> = fake.with(|s| s.itens.iter().map(|i| i.id).collect());
    assert_eq!(ids, [1, 3]);
    assert_eq!(store.read_with(window, |s, _| s.selected()), None);
}

/// A raiz inteira numa janela de teste, com a fila e as linhas já lidas. As posições saem do
/// primeiro quadro: o quadro que reaproveita uma view em cache não repõe o `debug_bounds`.
fn fila_na_janela(
    cx: &mut TestAppContext,
) -> (Arc<FakePlayer>, Entity<Store>, &mut VisualTestContext) {
    let (fake, store) = assentada(cx, PlayerState::Loaded);
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));
    window.run_until_parked();
    (fake, store, window)
}

#[gpui::test]
fn clicar_numa_linha_da_janela_seleciona_e_toca(cx: &mut TestAppContext) {
    let (fake, store, window) = fila_na_janela(cx);
    assert_eq!(store.read_with(window, |s, _| s.selected()), Some(1));
    let linha = window.debug_bounds("linha-2").expect("linha-2");

    window.simulate_click(linha.center(), Modifiers::none());
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));

    assert_eq!(store.read_with(window, |s, _| s.selected()), Some(2));
    assert!(fake.chamou("select"));
    assert_eq!(fake.with(|s| s.indice_atual), Some(1));
}

#[gpui::test]
fn clicar_em_limpar_esvazia_a_fila(cx: &mut TestAppContext) {
    let (fake, store, window) = fila_na_janela(cx);
    let limpar = window.debug_bounds("limpar").expect("limpar");

    window.simulate_click(limpar.center(), Modifiers::none());
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));
    let esvaziou = tique_ate(window, &store, |s| s.model().queue.items.is_empty());
    window.run_until_parked();

    assert!(fake.with(|s| s.itens.is_empty()));
    assert!(esvaziou);
    assert!(
        window.debug_bounds("fila-vazia").is_some(),
        "a fila não mostrou o estado vazio"
    );
}
