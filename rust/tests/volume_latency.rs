//! O volume pedido pelo slider tem de chegar ao hardware sem atraso perceptível.
//!
//! O defeito que este teste tranca: `write_volume_confirmed` dormia 30 ms fixos antes de
//! conferir *toda* escrita, inclusive as interativas. Medido neste Mac: o HAL assume o valor
//! e a leitura seguinte já confirma (0,17 ms); a espera não esperava nada. Como o slider
//! chama `set_volume` continuamente durante o arrasto, o custo virava 30 ms × N e o som só
//! mudava bem depois de o usuário soltar o controle.

use hog_audio::engine::Engine;
use hog_audio::fixtures;
use std::time::Instant;

/// Um arrasto curto de slider. O SwiftUI emite um pedido por evento de mouse.
const REQUESTS: usize = 20;

/// Medido com a correção: ~5 ms por pedido (o `set` do Core Audio domina). Com o defeito:
/// 33,5 ms. O teto fica entre os dois, longe o bastante dos dois para não oscilar.
const MAX_MS_PER_REQUEST: f64 = 15.0;

#[test]
#[ignore = "precisa de um device de saída real; toma o device por alguns segundos"]
fn arrasto_do_slider_chega_ao_hardware_sem_atraso_perceptivel() {
    let fixture = fixtures::path("t96_longo.flac");

    let engine = Engine::new();
    engine.load(&fixture).expect("deveria carregar");
    engine.play().expect("deveria tocar");

    let inicio = Instant::now();
    for i in 0..REQUESTS {
        engine
            .set_volume(0.10 + (i as f32) * 0.01)
            .expect("set_volume");
    }
    let arrasto = inicio.elapsed();

    // O último valor pedido tem de ser o que ficou no hardware: rápido e errado não serve.
    let volume_final = engine.status().volume();
    engine.shutdown().expect("shutdown");

    let por_pedido = arrasto.as_secs_f64() * 1000.0 / REQUESTS as f64;
    eprintln!(
        "arrasto de {REQUESTS} pedidos: {:.1} ms ({por_pedido:.1} ms por pedido)",
        arrasto.as_secs_f64() * 1000.0
    );

    let alvo = 0.10 + (REQUESTS as f32 - 1.0) * 0.01;
    assert!(
        (volume_final - alvo).abs() < 0.02,
        "o hardware deveria ficar no último valor do arrasto ({alvo:.2}), ficou em {volume_final:.2}"
    );
    assert!(
        por_pedido < MAX_MS_PER_REQUEST,
        "cada pedido de volume custou {por_pedido:.1} ms (teto {MAX_MS_PER_REQUEST} ms); \
         o usuário vê o slider mexer e o som mudar bem depois"
    );
}
