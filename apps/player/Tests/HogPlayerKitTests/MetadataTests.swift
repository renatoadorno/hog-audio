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

@Test func leUmFlacRealComCapa() async {
    // Caminho relativo a apps/player, que é de onde `swift test` roda.
    let url = URL(fileURLWithPath: "../../musicas/Skyfall.flac")
    guard FileManager.default.fileExists(atPath: url.path) else {
        return // o arquivo não é versionado; sem ele não há o que verificar
    }
    let meta = await loadMetadata(from: url)
    #expect(meta.title == "Skyfall")
    #expect(meta.artist == "Adele")
    // O AVFoundation desmonta o bloco de imagem do FLAC e entrega JPEG puro: FF D8 é o
    // marcador de início. Verificado neste arquivo, 38.583 bytes.
    #expect(meta.artwork != nil)
    #expect(meta.artwork?.prefix(2).elementsEqual([0xFF, 0xD8]) == true)
}

@Test func leUmFlacRealSemCapa() async {
    let url = URL(fileURLWithPath: "../../musicas/10-House-of-Memories.flac")
    guard FileManager.default.fileExists(atPath: url.path) else { return }
    let meta = await loadMetadata(from: url)
    #expect(meta.title == "House of Memories")
    #expect(meta.artist == "Panic! At The Disco")
    #expect(meta.album == "Death of a Bachelor")
    #expect(meta.artwork == nil) // este arquivo não tem capa embutida, verificado com ffprobe
}
