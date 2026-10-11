use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use hog_audio::transitions::PlayerState;
use hog_player_model::controller::{Config, Controller};
use hog_player_testing::{FakePlayer, Gate, PRAZO, config_de_teste, controlador, itens};

fn tocando_com_volume(volume: Option<f32>) -> (Controller, Arc<FakePlayer>) {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&["/tmp/a.flac", "/tmp/b.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Playing;
        s.volume_real = volume;
    });
    let mut c = controlador(&fake);
    c.tick();
    (c, fake)
}

#[test]
fn o_volume_vai_de_zero_a_um_para_o_engine() {
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);
    c.apply_volume(0.35);
    assert!(c.wait_idle(PRAZO));
    assert_eq!(fake.with(|s| s.volume_aplicado), Some(0.35));
}

#[test]
fn volume_que_falha_volta_ao_valor_do_device_e_nao_ao_pedido() {
    let fake = FakePlayer::new();
    fake.with(|s| s.falha_ao_aplicar_volume = Some("device recusou o volume".to_string()));
    let mut c = controlador(&fake);

    c.apply_volume(0.9);
    assert!(c.wait_idle(PRAZO));

    assert!(c.model().error.is_some());
    assert_eq!(c.model().volume.value, 0.5);
}

#[test]
fn chamadas_consecutivas_de_volume_aplicam_a_ultima_pedida() {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.escalar_com_atraso = Some(0.25);
        s.atraso_de_volume = Duration::from_millis(150);
    });
    let mut c = controlador(&fake);

    c.apply_volume(0.25);
    c.apply_volume(0.75);
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.volume_aplicado), Some(0.75));
}

#[test]
fn o_arrasto_do_slider_nao_vira_uma_escrita_por_quadro() {
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);

    for i in 0..50 {
        c.apply_volume(f64::from(i) / 50.0);
    }
    assert!(c.wait_idle(PRAZO));

    assert!(fake.with(|s| s.escritas_de_volume) < 50);
    assert_eq!(fake.with(|s| s.volume_aplicado), Some((49.0 / 50.0) as f32));
    assert_eq!(c.model().volume.value, 49.0 / 50.0);
}

#[test]
fn erro_de_volume_some_depois_de_um_comando_bem_sucedido() {
    let fake = FakePlayer::new();
    fake.with(|s| s.falha_ao_aplicar_volume = Some("device recusou o volume".to_string()));
    let mut c = controlador(&fake);

    c.apply_volume(0.9);
    assert!(c.wait_idle(PRAZO));
    assert!(c.model().error.is_some());

    fake.with(|s| s.falha_ao_aplicar_volume = None);
    c.apply_volume(0.4);
    assert!(c.wait_idle(PRAZO));
    assert_eq!(c.model().error, None);
}

#[test]
fn o_slider_segue_o_volume_mudado_por_fora() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));
    fake.with(|s| s.volume_real = Some(0.25));
    c.tick();
    assert_eq!(c.model().volume.value, 0.25);
}

#[test]
fn o_slider_nao_segue_o_volume_fora_de_tocar_ou_pausar() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));
    fake.with(|s| {
        s.estado = PlayerState::Loaded;
        s.volume_real = Some(0.25);
    });
    c.tick();
    assert_eq!(c.model().volume.value, 0.5);
}

#[test]
fn o_slider_nao_pisca_na_troca_de_faixa() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));
    fake.with(|s| s.atraso_de_selecao = Duration::from_millis(100));

    c.select(2);
    fake.with(|s| s.volume_real = Some(0.9));
    c.tick();
    assert_eq!(c.model().volume.value, 0.5);

    fake.with(|s| s.volume_real = Some(0.5));
    assert!(c.wait_idle(PRAZO));
}

#[test]
fn o_slider_nao_sobrescreve_um_arrasto() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));

    c.set_volume_editing(true);
    fake.with(|s| s.volume_real = Some(0.25));
    c.tick();
    assert_eq!(c.model().volume.value, 0.5);

    c.set_volume_editing(false);
    c.tick();
    assert_eq!(c.model().volume.value, 0.25);
}

#[test]
fn o_slider_nao_volta_enquanto_a_escrita_nao_chegou_ao_device() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));
    fake.with(|s| {
        s.escalar_com_atraso = Some(0.25);
        s.atraso_de_volume = Duration::from_millis(150);
    });

    c.apply_volume(0.25);
    c.tick();
    assert_eq!(c.model().volume.value, 0.25);

    assert!(c.wait_idle(PRAZO));
}

#[test]
fn device_sem_controle_de_volume_nao_mexe_no_slider() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));
    fake.with(|s| s.volume_real = None);
    c.tick();
    assert_eq!(c.model().volume.value, 0.5);
}

/// 0.35 em `f32` e de volta em `f64` vira 0.3499999…: o slider mostraria 34%.
#[test]
fn volume_igual_ao_do_device_nao_arredonda_o_slider() {
    let (mut c, _) = tocando_com_volume(Some(0.5));
    c.apply_volume(0.35);
    assert!(c.wait_idle(PRAZO));
    c.tick();
    assert_eq!(c.model().volume.value, 0.35);
}

#[test]
fn o_mudo_do_device_aparece_na_tela() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));
    fake.with(|s| s.mudo = true);
    c.tick();
    assert!(c.model().volume.muted);
}

/// Uma escrita já chegou ao player e está presa no portão; o pedido seguinte espera no slot. O
/// `shutdown` chega nesse meio: a escrita em curso termina, e o pedido que ainda não começou é
/// descartado, como os comandos, em vez de chegar a um player que já encerrou.
#[test]
fn shutdown_descarta_o_pedido_de_volume_que_ainda_nao_foi_escrito() {
    let fake = FakePlayer::new();
    let portao = Gate::new();
    fake.with(|s| s.portao_do_volume = Some(portao.clone()));
    let mut c = controlador(&fake);

    c.apply_volume(0.2);
    assert!(fake.esperar(PRAZO, |s| s.volume_esperando));
    c.apply_volume(0.3);
    c.shutdown().expect("shutdown");
    portao.open();
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.volume_depois_do_shutdown), 0);
    assert_eq!(fake.with(|s| s.volume_aplicado), Some(0.2));
    assert_eq!(c.model().error, None);
}

/// O `wake` conta só as chamadas da thread `hog-volume`, que vêm depois de a resposta estar no
/// canal. O dublê fica parado: fora de Playing e Paused o slider não segue o device, e só as
/// respostas das escritas decidem o valor.
fn contando_entregas_de_volume(fake: &Arc<FakePlayer>) -> (Controller, Arc<AtomicUsize>) {
    let entregas = Arc::new(AtomicUsize::new(0));
    let conta = entregas.clone();
    let config = Config {
        wake: Arc::new(move || {
            if std::thread::current().name() == Some("hog-volume") {
                conta.fetch_add(1, Ordering::SeqCst);
            }
        }),
        ..config_de_teste()
    };
    (Controller::new(fake.clone(), config), entregas)
}

/// Espera sem tique: um tique no meio drenaria a resposta antes da hora.
fn entregues(entregas: &AtomicUsize, alvo: usize) -> bool {
    let limite = Instant::now() + PRAZO;
    while entregas.load(Ordering::SeqCst) < alvo {
        if Instant::now() >= limite {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    true
}

/// A recusa chega ao tique depois de o usuário já ter pedido outro valor. O volume do device que
/// ela traz é de antes do pedido novo e não pode voltar ao slider.
#[test]
fn recusa_antiga_nao_desfaz_o_pedido_mais_novo() {
    let fake = FakePlayer::new();
    fake.with(|s| s.falha_ao_aplicar_volume = Some("device recusou o volume".to_string()));
    let (mut c, entregas) = contando_entregas_de_volume(&fake);

    c.apply_volume(0.9);
    assert!(entregues(&entregas, 1));
    fake.with(|s| s.falha_ao_aplicar_volume = None);
    c.apply_volume(0.4);
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.volume_real), Some(0.4));
    assert_eq!(c.model().volume.value, 0.4);
    assert_eq!(c.model().error, None);
}

/// A recusa e a escrita do pedido seguinte chegam ao mesmo tique. Drenadas as duas, nada mais está
/// a caminho: só saber qual pedido cada resposta fecha impede o slider de voltar ao valor da
/// recusa.
#[test]
fn recusa_e_escrita_nova_no_mesmo_tique_ficam_com_o_pedido_mais_novo() {
    let fake = FakePlayer::new();
    fake.with(|s| s.falha_ao_aplicar_volume = Some("device recusou o volume".to_string()));
    let (mut c, entregas) = contando_entregas_de_volume(&fake);

    c.apply_volume(0.9);
    assert!(entregues(&entregas, 1));
    fake.with(|s| s.falha_ao_aplicar_volume = None);
    c.apply_volume(0.4);
    assert!(entregues(&entregas, 2));
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.volume_real), Some(0.4));
    assert_eq!(c.model().volume.value, 0.4);
    assert_eq!(c.model().error, None);
}
