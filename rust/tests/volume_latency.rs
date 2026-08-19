//! O volume pedido pelo slider tem de chegar ao hardware sem atraso perceptivel.
//!
//! O defeito que este teste tranca: `write_volume_confirmed` dormia 30 ms fixos antes de
//! conferir *toda* escrita, inclusive as interativas. Medido neste Mac: o HAL assume o valor
//! e a leitura seguinte ja confirma (0,17 ms); a espera nao esperava nada. Como o slider
//! chama `set_volume` continuamente durante o arrasto, o custo virava 30 ms x N e o som so
//! mudava bem depois de o usuario soltar o controle.

use hog_audio::engine::Engine;
use std::time::Instant;

const FIXTURE: &str = "../testdata/t96_longo.flac";

/// Um arrasto curto de slider. O SwiftUI emite um pedido por evento de mouse.
const PEDIDOS: usize = 20;

/// Medido com a correcao: ~5 ms por pedido (o `set` do Core Audio domina). Com o defeito:
/// 33,5 ms. O teto fica entre os dois, longe o bastante dos dois para nao oscilar.
const TETO_MS_POR_PEDIDO: f64 = 15.0;

#[test]
#[ignore = "precisa de um device de saida real; toma o device por alguns segundos"]
fn arrasto_do_slider_chega_ao_hardware_sem_atraso_perceptivel() {
    assert!(std::path::Path::new(FIXTURE).exists(), "fixture ausente: {FIXTURE}");

    let engine = Engine::new();
    engine.load(FIXTURE).expect("deveria carregar");
    engine.play().expect("deveria tocar");

    let inicio = Instant::now();
    for i in 0..PEDIDOS {
        engine.set_volume(0.10 + (i as f32) * 0.01).expect("set_volume");
    }
    let arrasto = inicio.elapsed();

    // O ultimo valor pedido tem de ser o que ficou no hardware: rapido e errado nao serve.
    let volume_final = engine.status().volume();
    engine.shutdown().expect("shutdown");

    let por_pedido = arrasto.as_secs_f64() * 1000.0 / PEDIDOS as f64;
    eprintln!("arrasto de {PEDIDOS} pedidos: {:.1} ms ({por_pedido:.1} ms por pedido)",
              arrasto.as_secs_f64() * 1000.0);

    let alvo = 0.10 + (PEDIDOS as f32 - 1.0) * 0.01;
    assert!(
        (volume_final - alvo).abs() < 0.02,
        "o hardware deveria ficar no ultimo valor do arrasto ({alvo:.2}), ficou em {volume_final:.2}"
    );
    assert!(
        por_pedido < TETO_MS_POR_PEDIDO,
        "cada pedido de volume custou {por_pedido:.1} ms (teto {TETO_MS_POR_PEDIDO} ms); \
         o usuario ve o slider mexer e o som mudar bem depois"
    );
}
