#![allow(dead_code)]

use hog_audio::api::Snapshot;
use hog_audio::transitions::PlayerState;

/// Caminho de uma fixture de `testdata/` na raiz do repositório. Reprova se ela não existir:
/// um teste que depende de fixture não pode passar sem exercitá-la.
pub fn fixture(name: &str) -> String {
    let path = format!("{}/../../../testdata/{name}", env!("CARGO_MANIFEST_DIR"));
    assert!(
        std::path::Path::new(&path).exists(),
        "fixture ausente: {path}\nrode `make fixtures` na raiz do repositório"
    );
    path
}

pub fn snapshot(
    state: PlayerState,
    elapsed: f64,
    underruns: u64,
    current: Option<u32>,
    queue_len: u32,
) -> Snapshot {
    Snapshot {
        state,
        elapsed_seconds: elapsed,
        total_seconds: 208.7,
        underruns,
        volume_scalar: Some(0.5),
        muted: false,
        current_index: current,
        queue_len,
        queue_version: 0,
        device_suspect: false,
        problem_seq: 0,
    }
}
