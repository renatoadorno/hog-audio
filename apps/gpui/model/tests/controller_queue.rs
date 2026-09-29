use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hog_audio::transitions::PlayerState;
use hog_player_model::controller::{Config, Controller, EntryLoad};
use hog_player_model::entry::QueueEntryInfo;
use hog_player_model::media::tags::file_stem;
use hog_player_testing::{FakePlayer, PRAZO, config_de_teste, controlador, entrada, itens};

/// Um decodificador que entra em pânico (capa corrompida, por exemplo) não pode matar a thread de
/// carga. As quatro quebradas vêm na frente: sem a proteção, as quatro threads morreriam nelas, e
/// nenhuma outra linha carregaria.
#[test]
fn linha_que_entra_em_panico_vira_problema_e_as_outras_carregam() {
    let quebradas = [
        "/tmp/quebrada-0.flac",
        "/tmp/quebrada-1.flac",
        "/tmp/quebrada-2.flac",
        "/tmp/quebrada-3.flac",
    ];
    let boas = ["/tmp/boa-4.flac", "/tmp/boa-5.flac"];
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&[quebradas.as_slice(), boas.as_slice()].concat());
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Loaded;
    });
    let config = Config {
        load_entry: Arc::new(|path: &str| {
            if path.contains("quebrada") {
                panic!("pânico injetado na carga de {path}");
            }
            entrada(&format!("fila {path}"))
        }),
        ..config_de_teste()
    };
    let mut c = Controller::new(fake.clone(), config);

    assert!(c.wait_entries(PRAZO));

    for path in boas {
        assert_eq!(c.model().entries[path].title, format!("fila {path}"));
    }
    for path in quebradas {
        let linha = &c.model().entries[path];
        assert_eq!(linha.title, file_stem(path));
        assert_eq!(linha.problem.as_deref(), Some("não consegui ler o arquivo"));
    }
    c.select(6);
    assert!(c.wait_idle(PRAZO));
    assert_eq!(c.model().queue.current_item_id(), Some(6));
}

#[test]
fn a_fila_so_e_relida_quando_a_versao_muda() {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&["/tmp/a.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Loaded;
    });
    let mut c = controlador(&fake);

    c.tick();
    c.tick();
    assert_eq!(fake.with(|s| s.leituras_da_fila), 1);

    fake.acrescentar("/tmp/b.flac");
    fake.with(|s| s.versao_fila = 2);
    c.tick();
    assert_eq!(fake.with(|s| s.leituras_da_fila), 2);
    assert_eq!(c.model().queue.items.len(), 2);
}

#[test]
fn fila_ocupada_nao_bloqueia_e_rele_depois() {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&["/tmp/0.flac", "/tmp/1.flac", "/tmp/2.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Loaded;
    });
    let mut c = controlador(&fake);
    fake.acrescentar("/tmp/nova.flac");
    fake.with(|s| {
        s.versao_fila += 1;
        s.fila_ocupada = true;
    });

    c.tick();
    assert_eq!(c.model().queue.items.len(), 3);

    fake.with(|s| s.fila_ocupada = false);
    c.tick();
    assert_eq!(c.model().queue.items.len(), 4);
}

#[derive(Default)]
struct Registro {
    em_andamento: usize,
    pico: usize,
    ordem: Vec<String>,
}

/// O atraso só mantém as cargas em voo ao mesmo tempo; o teste espera a carga, nunca o relógio.
fn carregador(registro: Arc<Mutex<Registro>>) -> EntryLoad {
    Arc::new(move |path: &str| {
        {
            let mut r = registro.lock().unwrap();
            r.em_andamento += 1;
            r.pico = r.pico.max(r.em_andamento);
            r.ordem.push(path.to_string());
        }
        std::thread::sleep(Duration::from_millis(20));
        registro.lock().unwrap().em_andamento -= 1;
        QueueEntryInfo {
            quality: Some("24/96".to_string()),
            duration: Some("1:00".to_string()),
            ..entrada(&format!("tag de {path}"))
        }
    })
}

fn modelo(paths: &[&str], registro: &Arc<Mutex<Registro>>) -> (Controller, Arc<FakePlayer>) {
    let fake = FakePlayer::new();
    let config = Config {
        load_entry: carregador(registro.clone()),
        ..config_de_teste()
    };
    let mut c = Controller::new(fake.clone(), config);
    fake.with(|s| {
        s.itens = itens(paths);
        s.indice_atual = Some(0);
        s.estado = PlayerState::Loaded;
        s.versao_fila += 1;
    });
    c.tick();
    (c, fake)
}

#[test]
fn cada_faixa_que_entra_na_fila_ganha_sua_linha() {
    let registro = Arc::new(Mutex::new(Registro::default()));
    let paths = ["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"];
    let (mut c, _) = modelo(&paths, &registro);

    assert!(c.wait_entries(PRAZO));

    for path in paths {
        assert_eq!(c.model().entries[path].title, format!("tag de {path}"));
    }
}

#[test]
fn nunca_passa_de_quatro_cargas_ao_mesmo_tempo() {
    let registro = Arc::new(Mutex::new(Registro::default()));
    let owned: Vec<String> = (0..12).map(|i| format!("/tmp/faixa-{i}.flac")).collect();
    let paths: Vec<&str> = owned.iter().map(String::as_str).collect();
    let (mut c, _) = modelo(&paths, &registro);

    assert!(c.wait_entries(PRAZO));

    assert_eq!(registro.lock().unwrap().pico, 4);
    assert_eq!(c.model().entries.len(), 12);
}

#[test]
fn o_topo_da_fila_carrega_primeiro() {
    let registro = Arc::new(Mutex::new(Registro::default()));
    let owned: Vec<String> = (0..8).map(|i| format!("/tmp/faixa-{i}.flac")).collect();
    let paths: Vec<&str> = owned.iter().map(String::as_str).collect();
    let (mut c, _) = modelo(&paths, &registro);

    assert!(c.wait_entries(PRAZO));

    // Dentro de um lote de quatro a ordem de partida não é determinística; entre lotes é.
    let ordem = registro.lock().unwrap().ordem.clone();
    let primeiros: HashSet<&str> = ordem[..4].iter().map(String::as_str).collect();
    let ultimos: HashSet<&str> = ordem[4..].iter().map(String::as_str).collect();
    assert_eq!(
        primeiros,
        paths[..4].iter().copied().collect::<HashSet<_>>()
    );
    assert_eq!(ultimos, paths[4..].iter().copied().collect::<HashSet<_>>());
}

#[test]
fn tick_sem_mudanca_na_fila_nao_recarrega() {
    let registro = Arc::new(Mutex::new(Registro::default()));
    let (mut c, _) = modelo(&["/tmp/a.flac", "/tmp/b.flac"], &registro);
    assert!(c.wait_entries(PRAZO));

    c.tick();
    c.tick();
    assert!(c.wait_entries(PRAZO));

    assert_eq!(registro.lock().unwrap().ordem.len(), 2);
}

#[test]
fn faixa_nova_carrega_so_ela_mesma() {
    let registro = Arc::new(Mutex::new(Registro::default()));
    let (mut c, fake) = modelo(&["/tmp/a.flac"], &registro);
    assert!(c.wait_entries(PRAZO));

    fake.acrescentar("/tmp/b.flac");
    fake.with(|s| s.versao_fila += 1);
    c.tick();
    assert!(c.wait_entries(PRAZO));

    assert_eq!(
        registro.lock().unwrap().ordem,
        ["/tmp/a.flac", "/tmp/b.flac"]
    );
    assert_eq!(c.model().entries.len(), 2);
}

#[test]
fn limpar_a_fila_descarta_as_linhas() {
    let registro = Arc::new(Mutex::new(Registro::default()));
    let (mut c, _) = modelo(&["/tmp/a.flac", "/tmp/b.flac"], &registro);
    assert!(c.wait_entries(PRAZO));
    assert_eq!(c.model().entries.len(), 2);

    c.clear();
    assert!(c.wait_idle(PRAZO));

    assert!(c.model().entries.is_empty());
}

/// As linhas são por caminho, como no Swift: o mesmo arquivo duas vezes divide a linha, e
/// remover uma das duas não pode apagar os dados da outra.
#[test]
fn a_mesma_faixa_duas_vezes_divide_a_linha_e_remover_uma_nao_apaga_a_outra() {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&["/tmp/a.flac", "/tmp/a.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Loaded;
    });
    let mut c = controlador(&fake);
    assert!(c.wait_entries(PRAZO));
    assert_eq!(c.model().entries.len(), 1);

    c.remove(2);
    assert!(c.wait_idle(PRAZO));

    assert_eq!(c.model().queue.items.len(), 1);
    assert_eq!(c.model().entries["/tmp/a.flac"].title, "fila /tmp/a.flac");
}
