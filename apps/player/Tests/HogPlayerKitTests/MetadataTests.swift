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

// Os quatro testes abaixo cobrem o que os sintéticos de `org.id3`/`itsk` não cobrem: a função
// `convert`, traduzindo `AVMetadataItem` que veio de um arquivo de verdade. Um formato por
// teste, porque cada um guarda as tags de um jeito diferente — Vorbis comment no FLAC, átomos
// no M4A, ID3v2 no MP3 — e é exatamente aí que a tradução costuma escorregar.

@Test func leUmFlacRealComCapa() async throws {
    let meta = await loadMetadata(from: try fixture("meta_fixture.flac"))
    #expect(meta.title == "Titulo Teste")
    #expect(meta.artist == "Artista Teste")
    #expect(meta.album == "Album Teste")
    // O AVFoundation desmonta o bloco de imagem do FLAC e entrega JPEG puro: FF D8 é o
    // marcador de início.
    #expect(meta.artwork != nil)
    #expect(meta.artwork?.prefix(2).elementsEqual([0xFF, 0xD8]) == true)
}

@Test func leUmFlacRealSemCapa() async throws {
    let meta = await loadMetadata(from: try fixture("meta_fixture_sem_capa.flac"))
    #expect(meta.title == "Titulo Teste")
    #expect(meta.artist == "Artista Teste")
    #expect(meta.album == "Album Teste")
    #expect(meta.artwork == nil)
}

@Test func leUmM4aRealComCapa() async throws {
    let meta = await loadMetadata(from: try fixture("meta_fixture.m4a"))
    #expect(meta.title == "Titulo Teste")
    #expect(meta.artist == "Artista Teste")
    #expect(meta.album == "Album Teste")
    #expect(meta.artwork != nil)
}

@Test func leUmMp3RealComCapa() async throws {
    let meta = await loadMetadata(from: try fixture("meta_fixture.mp3"))
    #expect(meta.title == "Titulo Teste")
    #expect(meta.artist == "Artista Teste")
    #expect(meta.album == "Album Teste")
    #expect(meta.artwork != nil)
}
