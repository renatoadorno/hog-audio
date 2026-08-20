//! Resolve os caminhos dos arquivos de áudio que os testes consomem.
//!
//! Existe para que "a fixture não está aí" tenha **uma** resposta em todo o projeto: falhar,
//! dizendo o comando que a gera. Antes havia quatro respostas diferentes espalhadas — uma
//! delas um `return` silencioso que fazia o teste passar verde sem exercitar nada, que é
//! exatamente o defeito que um teste não pode ter.
//!
//! Fica na biblioteca, e não sob `#[cfg(test)]`, porque o teste de integração
//! (`tests/volume_latency.rs`) não enxerga o que é compilado só para os testes internos — e
//! uma política que não alcança todos os testes não é uma política.

/// Caminho de uma fixture a partir de `rust/`, que é de onde o `cargo test` roda.
///
/// # Panics
///
/// Se o arquivo não existir. É deliberado: um teste que depende de fixture e não a encontra
/// tem de reprovar, nunca passar por omissão.
pub fn path(name: &str) -> String {
    let path = format!("../testdata/{name}");
    assert!(
        std::path::Path::new(&path).exists(),
        "fixture ausente: {path}\nrode `make fixtures` na raiz do repositório para gerá-la"
    );
    path
}
