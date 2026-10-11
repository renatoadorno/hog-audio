use std::io::{ErrorKind, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::{Duration, Instant};

use hog_player_gpui::exit::{
    install_panic_hook, install_panic_hook_with, is_emergency, transition_log, watch_signals,
    watch_signals_with,
};
use hog_player_model::controller::LogSink;
use hog_player_testing::{FakePlayer, Gate, PRAZO};
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGUSR1};
use signal_hook::low_level::raise;

/// Espera a condição valer, com teto; devolve se valeu.
fn esperar(condicao: impl Fn() -> bool) -> bool {
    let limite = Instant::now() + PRAZO;
    while !condicao() && Instant::now() < limite {
        std::thread::sleep(Duration::from_millis(1));
    }
    condicao()
}

fn ligou(quit: &AtomicBool) -> bool {
    esperar(|| quit.load(Ordering::Acquire))
}

#[test]
fn o_log_de_transicoes_vai_para_o_arquivo() {
    let path = std::env::temp_dir().join(format!("hog-gpui-log-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&path);

    let log = transition_log(Some(path.clone()));
    log("seleção: espera 1 ms, comando 90 ms");

    let conteudo = std::fs::read_to_string(&path).expect("log");
    assert!(
        conteudo.contains("seleção: espera 1 ms, comando 90 ms"),
        "{conteudo}"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn sinal_pede_para_sair() {
    let quit = Arc::new(AtomicBool::new(false));
    watch_signals(Arc::clone(&quit)).expect("sinais");

    raise(SIGHUP).expect("raise");

    assert!(ligou(&quit));
}

/// `exp-sem-poll`: com o poll dormindo, o sinal acorda o Store, e a saída já está pedida quando o
/// tique do wake lê o pedido.
#[cfg(feature = "exp-sem-poll")]
#[test]
fn sem_poll_o_sinal_pede_para_sair_e_acorda_o_store() {
    use hog_player_gpui::exit::wake_on_signals;
    use hog_player_model::controller::Wake;

    let quit = Arc::new(AtomicBool::new(false));
    let (acordou, acordado) = mpsc::channel();
    let pedido = Arc::clone(&quit);
    let wake: Wake = Arc::new(move || {
        let _ = acordou.send(pedido.load(Ordering::Acquire));
    });
    wake_on_signals(Arc::clone(&quit), wake).expect("sinais");

    raise(SIGHUP).expect("raise");

    assert_eq!(acordado.recv_timeout(PRAZO), Ok(true));
}

#[test]
fn o_sighup_nunca_e_emergencia() {
    for depois in [
        Duration::ZERO,
        Duration::from_secs(1),
        Duration::from_secs(60),
    ] {
        assert!(!is_emergency(SIGHUP, depois), "{depois:?}");
    }
}

#[test]
fn repeticao_em_menos_de_1_s_nao_e_emergencia() {
    for sinal in [SIGINT, SIGTERM, SIGQUIT] {
        assert!(!is_emergency(sinal, Duration::ZERO), "{sinal}");
        assert!(!is_emergency(sinal, Duration::from_millis(999)), "{sinal}");
    }
}

#[test]
fn repeticao_depois_de_1_s_e_emergencia() {
    for sinal in [SIGINT, SIGTERM, SIGQUIT] {
        assert!(is_emergency(sinal, Duration::from_secs(1)), "{sinal}");
        assert!(is_emergency(sinal, Duration::from_secs(5)), "{sinal}");
    }
}

/// A fiação, com o relógio roteirizado e a emergência injetada: o SIGUSR1, que ninguém mais
/// escuta aqui, chega em 0, 0,5 s e 1,5 s, e só o terceiro vira emergência. Prova também que a
/// thread continua registrada depois do primeiro sinal.
#[test]
fn a_repeticao_so_vira_emergencia_depois_de_1_s() {
    let quit = Arc::new(AtomicBool::new(false));
    let inicio = Instant::now();
    let roteiro = [0, 500, 1500].map(Duration::from_millis);
    let lidos = Arc::new(AtomicUsize::new(0));
    let relogio = {
        let lidos = Arc::clone(&lidos);
        move || inicio + roteiro[lidos.fetch_add(1, Ordering::SeqCst).min(2)]
    };
    let (tx, rx) = mpsc::channel();
    let emergencia = {
        let lidos = Arc::clone(&lidos);
        move |sinal| {
            let _ = tx.send((sinal, lidos.load(Ordering::SeqCst)));
        }
    };
    watch_signals_with(Arc::clone(&quit), &[SIGUSR1], relogio, emergencia).expect("sinais");

    // Um sinal por vez: dois pendentes juntos viram uma entrega só.
    for n in 1..=3 {
        raise(SIGUSR1).expect("raise");
        assert!(
            esperar(|| lidos.load(Ordering::SeqCst) >= n),
            "o sinal {n} não chegou"
        );
    }

    assert!(quit.load(Ordering::Acquire));
    assert_eq!(rx.recv_timeout(PRAZO), Ok((SIGUSR1, 3)));
}

/// Roda um dos testes `filho_*` deste binário num processo à parte: a saída de emergência de
/// verdade mata o processo, e o deste binário não pode morrer.
fn filho(teste: &str) -> Output {
    comando_do_filho(teste).output().expect("processo filho")
}

fn comando_do_filho(teste: &str) -> Command {
    let mut comando = Command::new(std::env::current_exe().expect("binário de teste"));
    comando.args([teste, "--exact", "--ignored", "--nocapture"]);
    comando
}

/// O `--exact` com um nome que não existe roda zero testes e sai com 0: sem a contagem, um filho
/// renomeado passaria sem rodar nada.
fn passou_sozinho(saida: &Output) {
    let stdout = String::from_utf8_lossy(&saida.stdout);
    assert!(saida.status.success(), "{:?}\n{stdout}", saida.status);
    assert!(stdout.contains("1 passed"), "o filho não rodou: {stdout}");
}

/// Dois sinais com `watch_signals` de verdade, o segundo `intervalo` depois de o primeiro ligar
/// a flag; depois, tempo de sobra para a emergência agir antes de o teste terminar.
fn dois_sinais(sinal: i32, intervalo: Duration) {
    let quit = Arc::new(AtomicBool::new(false));
    watch_signals(Arc::clone(&quit)).expect("sinais");

    raise(sinal).expect("raise");
    assert!(ligou(&quit));
    std::thread::sleep(intervalo);
    raise(sinal).expect("raise");

    std::thread::sleep(Duration::from_millis(300));
}

#[test]
#[ignore = "roda só como processo filho de o_sigterm_repetido_depois_de_1_s_encerra_o_processo"]
fn filho_sigterm_depois_de_1_s() {
    dois_sinais(SIGTERM, Duration::from_millis(1100));
}

#[test]
fn o_sigterm_repetido_depois_de_1_s_encerra_o_processo() {
    let status = filho("filho_sigterm_depois_de_1_s").status;
    assert_eq!(status.signal(), Some(SIGTERM), "{status:?}");
}

#[test]
#[ignore = "roda só como processo filho de o_sigterm_repetido_em_menos_de_1_s_nao_encerra"]
fn filho_sigterm_em_menos_de_1_s() {
    dois_sinais(SIGTERM, Duration::ZERO);
}

#[test]
fn o_sigterm_repetido_em_menos_de_1_s_nao_encerra() {
    passou_sozinho(&filho("filho_sigterm_em_menos_de_1_s"));
}

/// Fechar a aba do terminal: dois SIGHUP. O intervalo passa de 1 s de propósito, para que só a
/// regra do SIGHUP segure a emergência.
#[test]
#[ignore = "roda só como processo filho de dois_sighup_nao_encerram_o_processo"]
fn filho_dois_sighup() {
    dois_sinais(SIGHUP, Duration::from_millis(1100));
}

#[test]
fn dois_sighup_nao_encerram_o_processo() {
    passou_sozinho(&filho("filho_dois_sighup"));
}

#[test]
#[ignore = "roda só como processo filho de log_sem_arquivo_avisa_uma_vez_no_stderr"]
fn filho_log_sem_arquivo() {
    let log = transition_log(Some(PathBuf::from("/hog-gpui-pasta-que-nao-existe/x.log")));
    log("linha um");
    log("linha dois");
}

/// Num filho, porque o stderr do próprio processo não dá para ler daqui.
#[test]
fn log_sem_arquivo_avisa_uma_vez_no_stderr() {
    let saida = filho("filho_log_sem_arquivo");
    let stderr = String::from_utf8_lossy(&saida.stderr);

    passou_sozinho(&saida);
    assert_eq!(stderr.matches("não abri").count(), 1, "{stderr}");
    assert!(stderr.contains("linha dois"), "{stderr}");
}

#[test]
#[ignore = "roda só como processo filho de o_terminal_fechado_nao_derruba_o_log"]
fn filho_log_com_stderr_quebrado() {
    // O pai fecha a ponta de leitura do stderr antes de soltar o stdin.
    std::io::stdin()
        .read_to_end(&mut Vec::new())
        .expect("stdin do pai");
    let quebrado = std::io::stderr().write_all(b"sonda\n");
    assert_eq!(
        quebrado.map_err(|e| e.kind()),
        Err(ErrorKind::BrokenPipe),
        "a montagem não quebrou o stderr"
    );
    println!("stderr quebrado");

    // Sem arquivo: o aviso de não ter aberto e a linha vão só para o stderr quebrado.
    let log = transition_log(Some(PathBuf::from("/hog-gpui-pasta-que-nao-existe/x.log")));
    log("linha com o terminal fechado");
}

/// Fechar o terminal deixa o stderr sem leitor (EPIPE). O log roda no `tick()`, na main, e no
/// panic hook, onde um pânico vira abort: escrever no stderr não pode entrar em pânico.
#[test]
fn o_terminal_fechado_nao_derruba_o_log() {
    let mut filho = comando_do_filho("filho_log_com_stderr_quebrado")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("processo filho");

    drop(filho.stderr.take());
    drop(filho.stdin.take());
    let saida = filho.wait_with_output().expect("saída do filho");

    assert!(
        String::from_utf8_lossy(&saida.stdout).contains("stderr quebrado"),
        "a montagem não quebrou o stderr"
    );
    passou_sozinho(&saida);
}

/// O panic hook é global ao processo: quem o instala roda sozinho e devolve o hook padrão no fim.
struct HookExclusivo {
    _vez: MutexGuard<'static, ()>,
}

impl Drop for HookExclusivo {
    fn drop(&mut self) {
        // `take_hook` entra em pânico numa thread em pânico: se o teste reprovou, o hook fica.
        if !std::thread::panicking() {
            drop(std::panic::take_hook());
        }
    }
}

fn hook_exclusivo() -> HookExclusivo {
    static VEZ: Mutex<()> = Mutex::new(());
    HookExclusivo {
        _vez: VEZ.lock().unwrap_or_else(|p| p.into_inner()),
    }
}

fn log_em(nome: &str) -> (PathBuf, LogSink) {
    let path = std::env::temp_dir().join(format!("hog-gpui-{nome}-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let log = transition_log(Some(path.clone()));
    (path, log)
}

fn ler(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn principal() -> bool {
    true
}

/// Com o predicado dizendo "é a principal": primeiro o caminho normal (o device é devolvido),
/// depois o engine travado (o hook desiste no teto). O resultado vai para o arquivo.
#[test]
fn o_panico_tenta_devolver_o_device_com_teto() {
    let _hook = hook_exclusivo();
    let (path, log) = log_em("panico-teto");

    let fake = FakePlayer::new();
    install_panic_hook_with(fake.clone(), log.clone(), principal);
    let _ = std::thread::spawn(|| panic!("pânico de teste")).join();
    assert!(fake.chamou("shutdown"));
    assert!(
        ler(&path).contains("pânico: device devolvido"),
        "{}",
        ler(&path)
    );

    // Um hook novo: o primeiro já gastou a tentativa única.
    let travado = FakePlayer::new();
    let portao = Gate::new();
    travado.with(|s| s.portao_do_shutdown = Some(portao.clone()));
    install_panic_hook_with(travado.clone(), log, principal);
    let antes = Instant::now();
    let _ = std::thread::spawn(|| panic!("pânico com o engine travado")).join();
    assert!(
        antes.elapsed() < Duration::from_millis(1500),
        "o hook não respeitou o teto"
    );
    portao.open();
    assert!(
        ler(&path).contains("pânico: o engine não respondeu em 500 ms"),
        "{}",
        ler(&path)
    );
    std::fs::remove_file(path).unwrap();
}

/// Uma capa corrompida não pode matar o player: o `catch_unwind` das cargas recupera o pânico,
/// e a janela continua aberta.
#[test]
fn o_panico_recuperado_fora_da_principal_nao_devolve_o_device() {
    let _hook = hook_exclusivo();
    let (path, log) = log_em("panico-recuperado");
    let fake = FakePlayer::new();
    install_panic_hook(fake.clone(), log);

    let recuperado = std::panic::catch_unwind(|| panic!("capa corrompida"));

    assert!(recuperado.is_err());
    assert!(!fake.chamou("shutdown"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn um_panico_na_restauracao_nao_reentra() {
    let _hook = hook_exclusivo();
    let (path, log) = log_em("panico-reentrada");
    let fake = FakePlayer::new();
    fake.with(|s| s.panico_no_shutdown = true);
    install_panic_hook_with(fake.clone(), log, principal);

    let _ = std::thread::spawn(|| panic!("pânico de teste")).join();

    let tentativas = fake.with(|s| s.chamadas.iter().filter(|c| *c == "shutdown").count());
    assert_eq!(tentativas, 1);
    assert!(
        ler(&path).contains("pânico: a restauração falhou ou entrou em pânico"),
        "{}",
        ler(&path)
    );
    std::fs::remove_file(path).unwrap();
}
