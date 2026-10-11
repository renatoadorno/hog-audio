use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use hog_player_model::audio_files::audio_files;

fn pasta(nome: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("hog-gpui-{nome}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("pasta");
    root
}

fn nomes(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// Liga a flag de oculto do Finder (`UF_HIDDEN`). Reprova se não conseguir: sem a flag, o teste
/// passaria sem exercitar nada.
fn ocultar(path: &Path) {
    let status = Command::new("chflags")
        .arg("hidden")
        .arg(path)
        .status()
        .expect("rodar chflags");
    assert!(
        status.success(),
        "chflags hidden falhou em {}",
        path.display()
    );
}

#[test]
fn expande_pastas_aninhadas_filtra_e_ordena_audio() {
    let root = pasta("ingestao");
    let nested = root.join("CD2");
    // `a/` contra `a-b/`: por bytes `-` vem antes de `/`, como no Swift; por componente, `a` vem
    // antes de `a-b`. Sem as duas, qualquer ordenação passaria.
    for dir in [&nested, &root.join("a"), &root.join("a-b")] {
        fs::create_dir_all(dir).unwrap();
    }
    for path in [
        root.join("02-song.wav"),
        root.join("cover.jpg"),
        nested.join("01-song.flac"),
        nested.join("notes.txt"),
        root.join("a").join("y.flac"),
        root.join("a-b").join("x.flac"),
    ] {
        fs::write(path, b"").unwrap();
    }

    let result = audio_files(std::slice::from_ref(&root));

    assert_eq!(
        nomes(&result),
        ["02-song.wav", "01-song.flac", "x.flac", "y.flac"]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn lista_vazia_ou_sem_audio_devolve_vazio() {
    let root = pasta("sem-audio");
    fs::write(root.join("cover.png"), b"").unwrap();

    assert!(audio_files(&[]).is_empty());
    assert!(audio_files(std::slice::from_ref(&root)).is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn extensao_em_maiuscula_vale_e_oculto_nao() {
    let root = pasta("caixa");
    fs::write(root.join("FAIXA.FLAC"), b"").unwrap();
    fs::write(root.join(".oculta.flac"), b"").unwrap();

    assert_eq!(
        nomes(&audio_files(std::slice::from_ref(&root))),
        ["FAIXA.FLAC"]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn arquivo_solto_e_caminho_inexistente() {
    let root = pasta("solto");
    let faixa = root.join("a.m4a");
    fs::write(&faixa, b"").unwrap();

    let result = audio_files(&[faixa.clone(), root.join("nao-existe.flac")]);

    assert_eq!(result, [faixa]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn oculto_pelo_finder_fica_de_fora() {
    let root = pasta("finder");
    let arquivo = root.join("finder-oculto.flac");
    let escondida = root.join("pasta-oculta-finder");
    fs::create_dir_all(&escondida).unwrap();
    for path in [
        arquivo.clone(),
        escondida.join("dentro.flac"),
        root.join("visivel.flac"),
    ] {
        fs::write(path, b"").unwrap();
    }
    ocultar(&arquivo);
    ocultar(&escondida);

    assert_eq!(
        nomes(&audio_files(std::slice::from_ref(&root))),
        ["visivel.flac"]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn symlink_de_arquivo_dentro_da_pasta_fica_de_fora() {
    let root = pasta("link-dentro");
    let sub = root.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(root.join("visivel.flac"), b"").unwrap();
    symlink("../visivel.flac", sub.join("link-arquivo.flac")).unwrap();

    assert_eq!(
        nomes(&audio_files(std::slice::from_ref(&root))),
        ["visivel.flac"]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn symlink_de_arquivo_passado_direto_fica_de_fora() {
    let root = pasta("link-solto");
    let faixa = root.join("real.flac");
    let link = root.join("link-solto.flac");
    fs::write(&faixa, b"").unwrap();
    symlink("real.flac", &link).unwrap();

    assert_eq!(audio_files(&[link, faixa.clone()]), [faixa]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn symlink_de_pasta_passado_direto_fica_de_fora() {
    let root = pasta("link-pasta");
    let real = root.join("pasta-real");
    let link = root.join("link-pasta");
    fs::create_dir_all(&real).unwrap();
    fs::write(real.join("x.flac"), b"").unwrap();
    symlink("pasta-real", &link).unwrap();

    assert!(audio_files(&[link]).is_empty());
    assert_eq!(nomes(&audio_files(&[real])), ["x.flac"]);
    fs::remove_dir_all(root).unwrap();
}
