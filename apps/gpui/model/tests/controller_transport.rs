use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hog_audio::transitions::PlayerState;
use hog_player_model::controller::{Config, Controller, MetadataLoad};
use hog_player_model::display::NO_TRACK_TITLE;
use std::sync::atomic::{AtomicUsize, Ordering};

use hog_player_testing::{
    FakePlayer, Gate, PRAZO, config_de_teste, controlador, entrada, itens, metadados, pasta_com,
};

fn carregada(paths: &[&str], estado: PlayerState) -> Arc<FakePlayer> {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(paths);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = estado;
    });
    fake
}

fn tocou(fake: &FakePlayer) -> bool {
    fake.esperar(PRAZO, |s| s.chamadas.iter().any(|c| c == "play"))
}

#[test]
fn o_botao_toca_quando_parado_e_pausa_quando_tocando() {
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);

    fake.with(|s| s.estado = PlayerState::Loaded);
    c.tick();
    c.toggle();
    assert!(c.wait_idle(PRAZO));
    assert!(fake.chamou("play"));

    fake.with(|s| s.estado = PlayerState::Playing);
    c.tick();
    c.toggle();
    assert!(c.wait_idle(PRAZO));
    assert!(fake.chamou("pause"));
}

#[test]
fn sem_faixa_carregada_o_botao_nao_faz_nada() {
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);
    c.tick();
    c.toggle();
    assert!(c.wait_idle(PRAZO));
    assert!(fake.with(|s| s.chamadas.is_empty()));
}

#[test]
fn o_tick_atualiza_o_que_a_tela_mostra() {
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);
    fake.with(|s| s.estado = PlayerState::Playing);
    c.tick();
    assert!(c.model().display.is_playing);
    assert!(c.model().display.can_pause);
}

/// Com outro comando na fila atrás do toggle, o acompanhamento do volume externo está travado:
/// só a releitura do fim do toggle pode trazer o volume real.
#[test]
fn o_toggle_rele_o_volume_real_do_device_ao_fim_do_comando() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Loaded);
    let portao = Gate::new();
    fake.with(|s| {
        s.volume_real = Some(0.82);
        s.portao_da_selecao = Some(portao.clone());
    });
    let mut c = controlador(&fake);
    c.tick();

    c.toggle();
    c.select(2);
    assert!(fake.esperar(PRAZO, |s| s.selecao_esperando));
    c.tick();

    assert_eq!(c.model().volume.value, f64::from(0.82_f32));
    portao.open();
    assert!(c.wait_idle(PRAZO));
}

#[test]
fn finished_dispara_um_unico_avanco_enquanto_a_carga_corre() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Finished);
    fake.with(|s| s.atraso_de_avanco = Duration::from_millis(150));
    let mut c = controlador(&fake);

    for _ in 0..6 {
        c.tick();
    }
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.avancos), 1);
    assert_eq!(c.model().queue.current_index, Some(1));
}

#[test]
fn next_manual_pendente_impede_auto_avanco_do_mesmo_finished() {
    let fake = carregada(
        &["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"],
        PlayerState::Finished,
    );
    let mut c = controlador(&fake);

    c.next();
    c.tick();
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.avancos), 0);
    assert_eq!(c.model().queue.current_index, Some(1));
}

#[test]
fn selecao_e_play_preservam_a_ordem_de_chegada() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Loaded);
    fake.with(|s| s.atraso_de_selecao = Duration::from_millis(100));
    let mut c = controlador(&fake);

    c.select(2);
    c.toggle();
    assert!(c.wait_idle(PRAZO));

    let chamadas = fake.with(|s| s.chamadas.clone());
    assert_eq!(chamadas[chamadas.len() - 2..], ["select", "play"]);
    assert_eq!(
        fake.with(|s| (s.indice_atual, s.estado)),
        (Some(1), PlayerState::Playing)
    );
}

/// O play fica preso no portão segurando a thread de comandos; a seleção espera a vez atrás
/// dele. O shutdown chega nesse meio: o que ainda não começou é descartado.
#[test]
fn shutdown_descarta_o_que_ainda_nao_chegou_ao_player() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Loaded);
    let portao = Gate::new();
    fake.with(|s| s.portao_do_play = Some(portao.clone()));
    let mut c = controlador(&fake);

    c.toggle();
    assert!(tocou(&fake));
    c.select(2);
    c.shutdown().expect("shutdown");
    portao.open();
    assert!(c.wait_idle(PRAZO));

    assert!(!fake.chamou("select"));
    assert!(fake.chamou("shutdown"));
    assert_eq!(fake.with(|s| s.indice_atual), Some(0));
}

#[test]
fn remocao_classifica_a_faixa_atual_depois_da_selecao_pendente() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Loaded);
    fake.with(|s| s.atraso_de_selecao = Duration::from_millis(100));
    let mut c = controlador(&fake);

    c.select(2);
    c.remove(2);
    assert!(c.wait_idle(PRAZO));

    assert_eq!(c.model().queue.current_index, None);
    assert_eq!(c.model().display.title, NO_TRACK_TITLE);
    let paths: Vec<_> = c
        .model()
        .queue
        .items
        .iter()
        .map(|t| t.path.as_str())
        .collect();
    assert_eq!(paths, ["/tmp/a.flac"]);
}

#[test]
fn duas_remocoes_seguidas_apagam_as_faixas_certas() {
    let fake = carregada(
        &["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"],
        PlayerState::Loaded,
    );
    let mut c = controlador(&fake);

    c.remove(2);
    c.remove(3);
    assert!(c.wait_idle(PRAZO));

    let paths: Vec<_> = fake.with(|s| s.itens.iter().map(|i| i.path.clone()).collect());
    assert_eq!(paths, ["/tmp/a.flac"]);
    assert_eq!(c.model().error, None);
}

#[test]
fn remover_a_faixa_atual_nao_carrega_de_novo_quem_assumiu() {
    let fake = carregada(
        &["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"],
        PlayerState::Playing,
    );
    let mut c = controlador(&fake);

    c.remove(1);
    assert!(c.wait_idle(PRAZO));

    assert!(!fake.chamou("select"));
    assert_eq!(c.model().queue.current_index, Some(0));
    assert_eq!(c.model().queue.current_item_id(), Some(2));
    assert!(c.model().display.technical_line.contains("44.1 kHz"));
}

#[test]
fn problema_do_device_aparece_uma_vez_so() {
    let fake = carregada(&["/tmp/a.flac"], PlayerState::Playing);
    let mut c = controlador(&fake);

    fake.with(|s| {
        s.problema = Some("o device sumiu".to_string());
        s.seq_problema = 1;
    });
    c.tick();
    assert_eq!(c.model().error.as_deref(), Some("o device sumiu"));

    c.apply_volume(0.3);
    assert!(c.wait_idle(PRAZO));
    assert_eq!(c.model().error, None);
    c.tick();
    assert_eq!(c.model().error, None);
}

#[test]
fn aviso_publicado_no_play_nao_some_quando_o_play_termina() {
    let fake = carregada(&["/tmp/a.flac"], PlayerState::Loaded);
    let portao = Gate::new();
    fake.with(|s| {
        s.aviso_ao_tocar = Some("o volume voltou ao teto".to_string());
        s.portao_do_play = Some(portao.clone());
    });
    let mut c = controlador(&fake);
    c.tick();

    c.toggle();
    assert!(fake.esperar(PRAZO, |s| s.seq_problema > 0));
    c.tick();
    assert_eq!(c.model().error.as_deref(), Some("o volume voltou ao teto"));

    portao.open();
    assert!(c.wait_idle(PRAZO));
    assert_eq!(c.model().error.as_deref(), Some("o volume voltou ao teto"));
}

#[test]
fn problema_de_antes_de_abrir_nao_aparece() {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.problema = Some("de uma sessão anterior".to_string());
        s.seq_problema = 3;
    });
    let mut c = controlador(&fake);
    c.tick();
    assert_eq!(c.model().error, None);
}

#[test]
fn device_suspeito_no_fim_da_faixa_recupera_e_nao_avanca() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Finished);
    fake.with(|s| s.suspeita_de_device = true);
    let mut c = controlador(&fake);

    c.tick();
    assert!(c.wait_idle(PRAZO));

    assert_eq!(
        fake.with(|s| (s.recuperacoes, s.avancos, s.estado)),
        (1, 0, PlayerState::Loaded)
    );
    assert!(
        c.model()
            .error
            .as_deref()
            .is_some_and(|e| e.contains("desconectado"))
    );
}

/// O play preso no portão segura a thread de comandos durante os três tiques. Sem isso, a
/// recuperação do dublê pode rodar entre dois tiques e apagar a suspeita, e o tique seguinte não
/// teria o que repetir: o teste passaria sem o guard.
#[test]
fn suspeita_repetida_no_tick_nao_enfileira_outra_recuperacao() {
    let fake = carregada(&["/tmp/a.flac"], PlayerState::Loaded);
    let portao = Gate::new();
    fake.with(|s| s.portao_do_play = Some(portao.clone()));
    let mut c = controlador(&fake);
    c.toggle();
    assert!(tocou(&fake));
    fake.with(|s| s.suspeita_de_device = true);

    c.tick();
    c.tick();
    c.tick();
    portao.open();
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.recuperacoes), 1);
}

#[test]
fn dois_toggles_rapidos_tocam_e_depois_pausam() {
    let fake = carregada(&["/tmp/a.flac"], PlayerState::Loaded);
    let mut c = controlador(&fake);

    c.toggle();
    c.toggle();
    assert!(c.wait_idle(PRAZO));

    let chamadas = fake.with(|s| s.chamadas.clone());
    assert_eq!(chamadas[chamadas.len() - 2..], ["play", "pause"]);
    assert_eq!(fake.with(|s| s.estado), PlayerState::Paused);
}

/// Cinco faixas carregadas, com os metadados vindos do `load` dado.
fn fila_com_metadados(load: MetadataLoad) -> (Controller, Arc<FakePlayer>) {
    let fake = carregada(
        &[
            "/tmp/0.flac",
            "/tmp/1.flac",
            "/tmp/2.flac",
            "/tmp/3.flac",
            "/tmp/4.flac",
        ],
        PlayerState::Loaded,
    );
    let config = Config {
        load_entry: Arc::new(|path: &str| hog_player_model::entry::QueueEntryInfo {
            artist: Some("Artista".to_string()),
            album: Some("Álbum".to_string()),
            ..entrada(&format!("fila {path}"))
        }),
        load_metadata: load,
        ..config_de_teste()
    };
    (Controller::new(fake.clone(), config), fake)
}

#[test]
fn next_nao_espera_os_metadados() {
    let portao = Gate::new();
    let segura = portao.clone();
    let (mut c, fake) = fila_com_metadados(Arc::new(move |path: &str| {
        segura.wait();
        metadados(&format!("meta {path}"))
    }));

    c.next();
    assert!(
        c.wait_idle(PRAZO),
        "o comando ficou esperando a leitura de metadados"
    );
    assert_eq!(fake.with(|s| s.navegacoes.clone()), [1]);
    assert_eq!(c.model().queue.current_index, Some(1));

    portao.open();
    assert!(c.wait_metadata(PRAZO));
    assert_eq!(c.model().display.title, "meta /tmp/1.flac");
}

#[test]
fn o_titulo_vem_da_fila_antes_dos_metadados() {
    let portao = Gate::new();
    let segura = portao.clone();
    let (mut c, _) = fila_com_metadados(Arc::new(move |path: &str| {
        segura.wait();
        metadados(&format!("meta {path}"))
    }));
    assert!(c.wait_entries(PRAZO));

    c.next();
    assert!(c.wait_idle(PRAZO));
    assert_eq!(c.model().display.title, "fila /tmp/1.flac");
    assert_eq!(c.model().display.artist, "Artista");

    portao.open();
    assert!(c.wait_metadata(PRAZO));
}

#[test]
fn metadado_atrasado_de_faixa_antiga_e_descartado() {
    let portao = Gate::new();
    let segura = portao.clone();
    let (mut c, _) = fila_com_metadados(Arc::new(move |path: &str| {
        if path == "/tmp/1.flac" {
            segura.wait();
        }
        metadados(&format!("meta {path}"))
    }));

    c.next();
    assert!(c.wait_idle(PRAZO));
    c.next();
    assert!(c.wait_idle(PRAZO));
    assert!(c.wait_until(PRAZO, |c| c.model().display.title == "meta /tmp/2.flac"));

    portao.open();
    assert!(c.wait_metadata(PRAZO));
    assert_eq!(c.model().display.title, "meta /tmp/2.flac");
}

/// As duas leituras entregam antes do mesmo tique, a da faixa antiga por último. O tique fica só
/// com a entrega mais recente de cada leva: sem a geração, seria a antiga, a conferência de
/// caminho a recusaria, e os metadados da faixa atual se perderiam. O `wake` conta só as chamadas
/// da thread `hog-metadata`, que vêm depois de o resultado estar no canal.
#[test]
fn metadado_antigo_entregue_no_mesmo_tique_nao_apaga_o_da_faixa_atual() {
    let (portao_1, portao_2) = (Gate::new(), Gate::new());
    let (segura_1, segura_2) = (portao_1.clone(), portao_2.clone());
    let entregas = Arc::new(AtomicUsize::new(0));
    let conta = entregas.clone();
    let fake = carregada(
        &["/tmp/0.flac", "/tmp/1.flac", "/tmp/2.flac"],
        PlayerState::Loaded,
    );
    let config = Config {
        load_metadata: Arc::new(move |path: &str| {
            match path {
                "/tmp/1.flac" => segura_1.wait(),
                "/tmp/2.flac" => segura_2.wait(),
                _ => {}
            }
            metadados(&format!("meta {path}"))
        }),
        wake: Arc::new(move || {
            if std::thread::current().name() == Some("hog-metadata") {
                conta.fetch_add(1, Ordering::SeqCst);
            }
        }),
        ..config_de_teste()
    };
    let mut c = Controller::new(fake.clone(), config);

    c.next();
    assert!(c.wait_idle(PRAZO));
    c.next();
    assert!(c.wait_idle(PRAZO));

    portao_2.open();
    assert!(entregues(&entregas, 1));
    portao_1.open();
    assert!(entregues(&entregas, 2));

    assert!(c.wait_metadata(PRAZO));
    assert_eq!(c.model().display.title, "meta /tmp/2.flac");
}

/// Espera sem tique: um tique no meio esvaziaria o canal entre as duas entregas.
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

/// Voltar a uma faixa cujos metadados já foram lidos não abre geração nova: a leitura da faixa de
/// onde se saiu chega com a geração corrente, e só a conferência de caminho a recusa.
#[test]
fn voltar_a_faixa_ja_lida_descarta_o_metadado_da_outra() {
    let portao = Gate::new();
    let segura = portao.clone();
    let (mut c, _) = fila_com_metadados(Arc::new(move |path: &str| {
        if path == "/tmp/2.flac" {
            segura.wait();
        }
        metadados(&format!("meta {path}"))
    }));

    c.next();
    assert!(c.wait_idle(PRAZO));
    assert!(c.wait_until(PRAZO, |c| c.model().display.title == "meta /tmp/1.flac"));
    c.next();
    assert!(c.wait_idle(PRAZO));
    c.previous();
    assert!(c.wait_idle(PRAZO));

    portao.open();
    assert!(c.wait_metadata(PRAZO));
    assert_eq!(c.model().queue.current_index, Some(1));
    assert_eq!(c.model().display.title, "meta /tmp/1.flac");
}

/// A leitura dos metadados entra em pânico (capa corrompida, por exemplo): a espera não pode
/// ficar presa, a faixa fica com o que a linha da fila mostra, e a troca seguinte carrega normal.
#[test]
fn metadado_que_entra_em_panico_nao_trava_a_espera() {
    let (mut c, _) = fila_com_metadados(Arc::new(|path: &str| {
        if path == "/tmp/1.flac" {
            panic!("pânico injetado nos metadados de {path}");
        }
        metadados(&format!("meta {path}"))
    }));
    assert!(c.wait_entries(PRAZO));

    c.next();
    assert!(c.wait_idle(PRAZO));
    assert!(c.wait_metadata(PRAZO));
    assert_eq!(c.model().display.title, "fila /tmp/1.flac");

    c.next();
    assert!(c.wait_idle(PRAZO));
    assert!(c.wait_metadata(PRAZO));
    assert_eq!(c.model().display.title, "meta /tmp/2.flac");
}

fn metadados_pelo_caminho() -> Config {
    Config {
        load_metadata: Arc::new(|path: &str| metadados(&format!("meta {path}"))),
        ..config_de_teste()
    }
}

/// O engine segura o lock da fila (numa aquisição do hog, por exemplo) no tique que aplica a
/// remoção, e a fila da tela fica defasada nesse tique. O pedido dos metadados de quem assumiu tem
/// de esperar a fila sincronizar: contra a fila velha, a atual ainda seria a removida.
#[test]
fn remover_a_atual_com_o_engine_ocupado_carrega_os_metadados_de_quem_assumiu() {
    let fake = carregada(
        &["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"],
        PlayerState::Playing,
    );
    let mut c = Controller::new(fake.clone(), metadados_pelo_caminho());
    assert!(c.wait_entries(PRAZO));
    c.select(1);
    assert!(c.wait_idle(PRAZO));
    assert!(c.wait_metadata(PRAZO));
    assert_eq!(c.model().display.title, "meta /tmp/a.flac");

    c.remove(1);
    assert!(fake.esperar(PRAZO, |s| s.itens.len() == 2));
    fake.with(|s| s.fila_ocupada = true);
    assert!(c.wait_idle(PRAZO));
    fake.with(|s| s.fila_ocupada = false);
    c.tick();
    assert!(c.wait_metadata(PRAZO));

    assert_eq!(c.model().queue.current_item_id(), Some(2));
    assert_eq!(c.model().display.title, "meta /tmp/b.flac");
}

/// Soltar numa fila vazia com o engine ocupado no tique que aplica a carga inicial. Contra a fila
/// velha (vazia) não há faixa atual, e esquecê-la apagaria também o formato recém-carregado.
#[test]
fn soltar_numa_fila_vazia_com_o_engine_ocupado_mantem_o_formato_e_carrega_os_metadados() {
    let pasta = pasta_com("soltar-ocupada", &["a.flac"]);
    let esperado = format!("meta {}", pasta.join("a.flac").display());
    let fake = FakePlayer::new();
    let mut c = Controller::new(fake.clone(), metadados_pelo_caminho());

    c.add(vec![pasta.clone()]);
    assert!(fake.esperar(PRAZO, |s| s.chamadas.iter().any(|x| x == "select")));
    fake.with(|s| s.fila_ocupada = true);
    assert!(c.wait_idle(PRAZO));
    fake.with(|s| s.fila_ocupada = false);
    c.tick();
    assert!(c.wait_metadata(PRAZO));

    let display = c.model().display.clone();
    std::fs::remove_dir_all(pasta).unwrap();
    assert!(
        display.technical_line.contains("44.1 kHz"),
        "linha técnica: {:?}",
        display.technical_line
    );
    assert_eq!(display.title, esperado);
}

/// Os metadados da atual chegam enquanto a fila da tela está defasada. Decididos contra a fila
/// velha, seriam recusados, e ninguém os pediria de novo: a remoção de outra faixa não recarrega
/// a atual.
#[test]
fn metadado_que_chega_com_a_fila_defasada_espera_a_fila_sincronizar() {
    let portao = Gate::new();
    let segura = portao.clone();
    let fake = carregada(
        &["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"],
        PlayerState::Playing,
    );
    let config = Config {
        load_metadata: Arc::new(move |path: &str| {
            if path == "/tmp/b.flac" {
                segura.wait();
            }
            metadados(&format!("meta {path}"))
        }),
        ..config_de_teste()
    };
    let mut c = Controller::new(fake.clone(), config);
    c.select(2);
    assert!(c.wait_idle(PRAZO));

    c.remove(1);
    assert!(fake.esperar(PRAZO, |s| s.itens.len() == 2));
    fake.with(|s| s.fila_ocupada = true);
    assert!(c.wait_idle(PRAZO));
    portao.open();
    assert!(c.wait_metadata(PRAZO));
    fake.with(|s| s.fila_ocupada = false);
    c.tick();

    assert_eq!(c.model().queue.current_item_id(), Some(2));
    assert_eq!(c.model().display.title, "meta /tmp/b.flac");
}

#[test]
fn tres_nexts_rapidos_viram_uma_carga_so() {
    let (mut c, fake) = fila_com_metadados(Arc::new(|path: &str| metadados(path)));
    let portao = Gate::new();
    fake.with(|s| s.portao_do_play = Some(portao.clone()));

    c.toggle();
    assert!(tocou(&fake));
    c.next();
    c.next();
    c.next();
    portao.open();
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.navegacoes.clone()), [3]);
    assert_eq!(c.model().queue.current_index, Some(3));
}

#[test]
fn next_e_previous_seguidos_nao_carregam_nada() {
    let (mut c, fake) = fila_com_metadados(Arc::new(|path: &str| metadados(path)));
    let portao = Gate::new();
    fake.with(|s| s.portao_do_play = Some(portao.clone()));

    c.toggle();
    assert!(tocou(&fake));
    c.next();
    c.previous();
    portao.open();
    assert!(c.wait_idle(PRAZO));

    assert!(fake.with(|s| s.navegacoes.is_empty()));
    assert_eq!(c.model().queue.current_index, Some(0));
}

#[test]
fn soltar_audio_numa_fila_vazia_seleciona_a_primeira() {
    let pasta = pasta_com("soltar-vazia", &["b.flac", "a.flac", "capa.jpg"]);
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);

    c.add(vec![pasta.clone()]);
    assert!(c.wait_idle(PRAZO));

    let paths: Vec<String> = fake.with(|s| s.itens.iter().map(|i| i.path.clone()).collect());
    assert_eq!(paths.len(), 2);
    assert!(
        paths[0].ends_with("/a.flac") && paths[1].ends_with("/b.flac"),
        "{paths:?}"
    );
    assert!(fake.chamou("select"));
    assert_eq!(c.model().error, None);
    std::fs::remove_dir_all(pasta).unwrap();
}

#[test]
fn soltar_numa_fila_com_faixas_nao_seleciona() {
    let pasta = pasta_com("soltar-cheia", &["c.flac"]);
    let fake = carregada(&["/tmp/a.flac"], PlayerState::Loaded);
    let mut c = controlador(&fake);

    c.add(vec![pasta.clone()]);
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.itens.len()), 2);
    assert!(!fake.chamou("select"));
    std::fs::remove_dir_all(pasta).unwrap();
}

#[test]
fn soltar_so_o_que_nao_e_audio_mostra_erro() {
    let pasta = pasta_com("soltar-sem-audio", &["capa.jpg"]);
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);

    c.add(vec![pasta.clone()]);
    assert!(c.wait_idle(PRAZO));

    assert_eq!(
        c.model().error.as_deref(),
        Some("nenhum arquivo de áudio encontrado")
    );
    assert!(fake.with(|s| s.itens.is_empty()));
    std::fs::remove_dir_all(pasta).unwrap();
}

#[test]
fn cada_transicao_gera_uma_linha_no_formato_do_swift() {
    let linhas = Arc::new(Mutex::new(Vec::<String>::new()));
    let guarda = linhas.clone();
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Loaded);
    let config = Config {
        log: Arc::new(move |linha: &str| guarda.lock().unwrap().push(linha.to_string())),
        ..config_de_teste()
    };
    let mut c = Controller::new(fake.clone(), config);

    let antes = Instant::now();
    c.select(2);
    assert!(c.wait_idle(PRAZO));
    assert!(antes.elapsed() < PRAZO);

    let linhas = linhas.lock().unwrap();
    assert_eq!(linhas.len(), 1, "{linhas:?}");
    assert!(linhas[0].starts_with("seleção: espera "), "{}", linhas[0]);
    assert!(
        linhas[0].contains(
            "(open 0.0, release 0.0, reopen 0.0, acquire 0.0, volume 0.0, prefill 0.0, start 0.0)"
        ),
        "{}",
        linhas[0]
    );
}

/// No Swift o fim de um comando atualiza a tela na hora (o `apply` chama `refresh()`). Aqui a
/// thread acorda quem desenha ao entregar a resposta, sem esperar o próximo tique do poll.
///
/// A fila entra no dublê depois do `new` e nenhum tique roda antes da asserção. Com a fila já na
/// construção, as threads de carga das linhas também acordam, e o teste passaria sem o `wake()`
/// da thread de comandos.
#[test]
fn cada_resposta_acorda_quem_desenha() {
    let acordou = Arc::new(AtomicUsize::new(0));
    let conta = acordou.clone();
    let fake = FakePlayer::new();
    let config = Config {
        wake: Arc::new(move || {
            conta.fetch_add(1, Ordering::SeqCst);
        }),
        ..config_de_teste()
    };
    let mut c = Controller::new(fake.clone(), config);
    fake.with(|s| {
        s.itens = itens(&["/tmp/a.flac", "/tmp/b.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Loaded;
    });
    let antes = acordou.load(Ordering::SeqCst);

    c.select(2);

    let limite = Instant::now() + PRAZO;
    while acordou.load(Ordering::SeqCst) == antes && Instant::now() < limite {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        acordou.load(Ordering::SeqCst) > antes,
        "a resposta do comando não acordou ninguém"
    );
    assert!(c.wait_idle(PRAZO));
}
