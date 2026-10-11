//! Expande pastas, descarta o que não é áudio e ordena por caminho: porte de `AudioFiles.swift`.
//! Roda na thread de comandos, nunca na main, porque uma pasta grande leva segundos.

use std::fs::{self, DirEntry};
use std::os::macos::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// O que o `ExtAudioFile` do core abre. O SwiftUI filtra por `UTType.audio`; aqui a lista é
/// explícita. A diferença é deliberada e fica registrada no resultado.
pub const AUDIO_EXTENSIONS: [&str; 9] = [
    "aac", "aif", "aifc", "aiff", "caf", "flac", "m4a", "mp3", "wav",
];

pub fn audio_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for path in paths {
        collect(path, &mut found);
    }
    // Por bytes do caminho inteiro, não por componente como o `Ord` de `PathBuf`. Coincide com o
    // `<` de String do Swift enquanto o nome não tiver acento decomposto (NFD): o Swift compara
    // por equivalência canônica, e normalizar aqui exigiria dependência nova.
    found.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    found
}

/// Symlink não entra, nem passado direto nem achado na pasta, como no Swift: de pasta criaria
/// ciclo, e de arquivo duplicaria a faixa quando aponta para dentro da árvore arrastada.
fn collect(path: &Path, found: &mut Vec<PathBuf>) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            if is_hidden(&entry) {
                continue;
            }
            let child = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => collect(&child, found),
                Ok(kind) if kind.is_file() && is_audio(&child) => found.push(child),
                _ => {}
            }
        }
    } else if meta.is_file() && is_audio(path) {
        found.push(path.to_path_buf());
    }
}

/// O `.skipsHiddenFiles` do Swift pula o nome com ponto e também a flag de oculto do Finder, que
/// o `~/Library` tem sem começar com ponto. Só vale dentro da pasta: o Swift aceita o oculto
/// passado direto.
fn is_hidden(entry: &DirEntry) -> bool {
    // `UF_HIDDEN` de <sys/stat.h>. Sem o `libc`, para não acrescentar dependência.
    const UF_HIDDEN: u32 = 0x8000;
    entry.file_name().to_string_lossy().starts_with('.')
        || entry
            .metadata()
            .is_ok_and(|meta| meta.st_flags() & UF_HIDDEN != 0)
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.as_str()))
}
