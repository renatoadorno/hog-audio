use std::sync::Arc;

use hog_audio::transitions::PlayerState;
use hog_player_model::controller::{Changes, Controller};
use hog_player_testing::{FakePlayer, PRAZO, controlador, itens};

fn tocando() -> (Controller, Arc<FakePlayer>) {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&["/tmp/a.flac", "/tmp/b.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Playing;
        s.decorrido = 1.0;
    });
    let mut c = controlador(&fake);
    assert!(c.wait_entries(PRAZO));
    assert!(c.wait_metadata(PRAZO));
    c.tick();
    (c, fake)
}

#[test]
fn mudanca_de_estado_marca_o_display() {
    let (mut c, fake) = tocando();
    fake.with(|s| s.estado = PlayerState::Paused);
    assert!(c.tick().display);
}

/// Tocando, o relógio muda a cada tique. Só ele pode ser marcado: capa, título e fila ficam como
/// estão. O tique cruza o segundo, para mudar o texto e a fração tocada.
#[test]
fn tique_do_relogio_so_marca_o_relogio() {
    let (mut c, fake) = tocando();
    fake.with(|s| s.decorrido = 2.0);
    assert_eq!(
        c.tick(),
        Changes {
            clock: true,
            ..Changes::NONE
        }
    );
}

#[test]
fn tick_sem_mudanca_nao_marca_nada() {
    let (mut c, _) = tocando();
    assert_eq!(c.tick(), Changes::NONE);
    assert_eq!(c.tick(), Changes::NONE);
}
