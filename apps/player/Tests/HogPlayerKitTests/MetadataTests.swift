import Testing
import Foundation
@testable import HogPlayerKit

private func item(_ keySpace: String, _ key: String, _ value: String) -> MetadataItem {
    MetadataItem(keySpace: keySpace, key: key, stringValue: value, dataValue: nil)
}

@Test func usaCommonMetadataQuandoDisponivel() {
    let common = [
        item("", "title", "Skyfall"),
        item("", "artist", "Adele"),
        item("", "albumName", "Skyfall"),
    ]
    let meta = trackMetadata(common: common, raw: [], fallbackFilename: "ignorado")
    #expect(meta.title == "Skyfall")
    #expect(meta.artist == "Adele")
    #expect(meta.album == "Skyfall")
}

@Test func caiNoVorbisQuandoCommonVemVazio() {
    // É exatamente o caso do FLAC: commonMetadata devolve lista vazia, medido.
    let raw = [
        item("vorb", "TITLE", "House of Memories"),
        item("vorb", "ARTIST", "Panic! At The Disco"),
        item("vorb", "ALBUM", "Death of a Bachelor"),
        item("vorb", "TRACKNUMBER", "10"),
    ]
    let meta = trackMetadata(common: [], raw: raw, fallbackFilename: "ignorado")
    #expect(meta.title == "House of Memories")
    #expect(meta.artist == "Panic! At The Disco")
    #expect(meta.album == "Death of a Bachelor")
}

@Test func leTagsDeId3() {
    let raw = [
        item("org.id3", "TIT2", "Titulo Teste"),
        item("org.id3", "TPE1", "Artista Teste"),
        item("org.id3", "TALB", "Album Teste"),
    ]
    let meta = trackMetadata(common: [], raw: raw, fallbackFilename: "ignorado")
    #expect(meta.title == "Titulo Teste")
    #expect(meta.artist == "Artista Teste")
    #expect(meta.album == "Album Teste")
}

@Test func semTagsUsaONomeDoArquivo() {
    let meta = trackMetadata(common: [], raw: [], fallbackFilename: "10-House-of-Memories")
    #expect(meta.title == "10-House-of-Memories")
    #expect(meta.artist == nil)
    #expect(meta.album == nil)
}

@Test func tagVaziaNaoVenceONomeDoArquivo() {
    let raw = [item("vorb", "TITLE", "   ")]
    let meta = trackMetadata(common: [], raw: raw, fallbackFilename: "faixa")
    #expect(meta.title == "faixa")
}

@Test func aCapaVemDoMetadataBlockPicture() {
    let jpeg = Data([0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10])
    let raw = [
        MetadataItem(keySpace: "vorb", key: "METADATA_BLOCK_PICTURE",
                     stringValue: nil, dataValue: jpeg)
    ]
    let meta = trackMetadata(common: [], raw: raw, fallbackFilename: "faixa")
    #expect(meta.artwork == jpeg)
}

// Caminhos relativos a apps/player, que é de onde `swift test` roda. Os arquivos não são
// versionados — `.enabled(if:)` faz o Swift Testing reportar os quatro testes abaixo como
// **skipped**, não como um "passou" que na verdade nunca rodou.
private let skyfallFlacPath = "../../musicas/Skyfall.flac"
private let houseOfMemoriesFlacPath = "../../musicas/10-House-of-Memories.flac"
private let metaFixtureM4aPath = "../../testdata/meta_fixture.m4a"
private let metaFixtureMp3Path = "../../testdata/meta_fixture.mp3"

@Test(.enabled(if: FileManager.default.fileExists(atPath: skyfallFlacPath)))
func leUmFlacRealComCapa() async {
    let meta = await loadMetadata(from: URL(fileURLWithPath: skyfallFlacPath))
    #expect(meta.title == "Skyfall")
    #expect(meta.artist == "Adele")
    // O AVFoundation desmonta o bloco de imagem do FLAC e entrega JPEG puro: FF D8 é o
    // marcador de início. Verificado neste arquivo, 38.583 bytes.
    #expect(meta.artwork != nil)
    #expect(meta.artwork?.prefix(2).elementsEqual([0xFF, 0xD8]) == true)
}

@Test(.enabled(if: FileManager.default.fileExists(atPath: houseOfMemoriesFlacPath)))
func leUmFlacRealSemCapa() async {
    let meta = await loadMetadata(from: URL(fileURLWithPath: houseOfMemoriesFlacPath))
    #expect(meta.title == "House of Memories")
    #expect(meta.artist == "Panic! At The Disco")
    #expect(meta.album == "Death of a Bachelor")
    #expect(meta.artwork == nil) // este arquivo não tem capa embutida, verificado com ffprobe
}

// As fixtures de M4A e MP3 abaixo cobrem o que os testes sintéticos de `org.id3`/`itsk`
// não cobrem: a função `convert` que traduz `AVMetadataItem` de verdade. Elas não são
// versionadas (mesma regra de `musicas/`) — para recriá-las:
//
//   ffmpeg -v error -y -f lavfi -i "sine=frequency=440:duration=2" \
//     -i musicas/Skyfall.flac -map 0:a -map 1:v -c:a alac -c:v copy \
//     -disposition:v attached_pic \
//     -metadata title="Titulo Teste" -metadata artist="Artista Teste" \
//     -metadata album="Album Teste" testdata/meta_fixture.m4a
//
//   ffmpeg -v error -y -f lavfi -i "sine=frequency=440:duration=2" \
//     -i musicas/Skyfall.flac -map 0:a -map 1:v -c:a libmp3lame -c:v copy \
//     -id3v2_version 3 \
//     -metadata title="Titulo Teste" -metadata artist="Artista Teste" \
//     -metadata album="Album Teste" testdata/meta_fixture.mp3

@Test(.enabled(if: FileManager.default.fileExists(atPath: metaFixtureM4aPath)))
func leUmM4aRealComCapa() async {
    let meta = await loadMetadata(from: URL(fileURLWithPath: metaFixtureM4aPath))
    #expect(meta.title == "Titulo Teste")
    #expect(meta.artist == "Artista Teste")
    #expect(meta.album == "Album Teste")
    #expect(meta.artwork != nil)
}

@Test(.enabled(if: FileManager.default.fileExists(atPath: metaFixtureMp3Path)))
func leUmMp3RealComCapa() async {
    let meta = await loadMetadata(from: URL(fileURLWithPath: metaFixtureMp3Path))
    #expect(meta.title == "Titulo Teste")
    #expect(meta.artist == "Artista Teste")
    #expect(meta.album == "Album Teste")
    #expect(meta.artwork != nil)
}
