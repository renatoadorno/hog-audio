import Foundation
import Testing

// Caminhos relativos a apps/player, que é de onde `swift test` roda. As fixtures não são
// versionadas — `make fixtures` as gera, a partir da receita em `tools/make_fixtures.sh`.
//
// Um arquivo ausente **reprova** o teste, mesma política do lado Rust (`rust/src/fixtures.rs`).
// Antes daqui saía um `.enabled(if:)`, que pulava calado: a suíte relatava sucesso sem ter
// exercitado o arquivo de verdade, que é justamente o que esses testes existem para cobrir.
func fixture(
    _ name: String,
    sourceLocation: SourceLocation = #_sourceLocation
) throws -> URL {
    let path = "../../testdata/\(name)"
    try #require(
        FileManager.default.fileExists(atPath: path),
        "fixture ausente: \(path) — rode `make fixtures` na raiz do repositório",
        sourceLocation: sourceLocation
    )
    return URL(fileURLWithPath: path)
}
