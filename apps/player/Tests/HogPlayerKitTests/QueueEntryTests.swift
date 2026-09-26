import CoreGraphics
import Foundation
import HogAudioBindings
@testable import HogPlayerKit
import Testing

private let tags = TrackMetadata(
    title: "Life in Technicolor", artist: "Coldplay",
    album: "Viva La Vida", artwork: nil
)

private let sonda = TrackProbe(
    sampleRate: 44100, bitDepth: 24, channels: 2, codec: "flac", totalSeconds: 149.6
)

@Test func seloCompactoUsaBitsBarraKilohertz() {
    #expect(compactQuality(sampleRate: 44100, bitDepth: 24) == "24/44.1")
    #expect(compactQuality(sampleRate: 96000, bitDepth: 24) == "24/96")
    #expect(compactQuality(sampleRate: 192_000, bitDepth: 24) == "24/192")
    #expect(compactQuality(sampleRate: 44100, bitDepth: 16) == "16/44.1")
}

@Test func subtituloJuntaArtistaEAlbumSoQuandoExistem() {
    let ambos = queueEntryInfo(metadata: tags, probe: .probed(sonda), thumbnail: nil)
    #expect(ambos.subtitle == "Coldplay — Viva La Vida")

    let soArtista = queueEntryInfo(
        metadata: TrackMetadata(title: "X", artist: "Coldplay", album: nil, artwork: nil),
        probe: .probed(sonda), thumbnail: nil
    )
    #expect(soArtista.subtitle == "Coldplay")

    let soAlbum = queueEntryInfo(
        metadata: TrackMetadata(title: "X", artist: nil, album: "Viva La Vida", artwork: nil),
        probe: .probed(sonda), thumbnail: nil
    )
    #expect(soAlbum.subtitle == "Viva La Vida")

    let nenhum = queueEntryInfo(
        metadata: TrackMetadata(title: "X", artist: nil, album: nil, artwork: nil),
        probe: .probed(sonda), thumbnail: nil
    )
    #expect(nenhum.subtitle.isEmpty)
}

@Test func sondagemBemSucedidaPreencheSeloTooltipEDuracao() {
    let info = queueEntryInfo(metadata: tags, probe: .probed(sonda), thumbnail: nil)
    #expect(info.title == "Life in Technicolor")
    #expect(info.quality == "24/44.1")
    #expect(info.qualityDetail == "44.1 kHz · 24 bit · flac")
    #expect(info.duration == "2:29")
    #expect(info.problem == nil)
}

@Test func sondagemQueFalhaViraProblemaSemQualidade() {
    let info = queueEntryInfo(
        metadata: tags, probe: .failed("não consegui abrir o arquivo"), thumbnail: nil
    )
    #expect(info.problem == "não consegui abrir o arquivo")
    #expect(info.quality == nil)
    #expect(info.duration == nil)
    // Os metadados continuam valendo: a linha ainda diz qual é a faixa.
    #expect(info.title == "Life in Technicolor")
}

@Test func miniaturaDaCapaRealNaoPassaDoTamanhoPedido() async throws {
    let meta = await loadMetadata(from: try fixture("meta_fixture.flac"))
    let artwork = try #require(meta.artwork)
    let thumbnail = try #require(makeThumbnail(from: artwork, maxPixelSize: 96))
    #expect(max(thumbnail.width, thumbnail.height) <= 96)
    #expect(max(thumbnail.width, thumbnail.height) > 0)
}

@Test func bytesQueNaoSaoImagemNaoViramMiniatura() {
    #expect(makeThumbnail(from: Data("não é imagem".utf8), maxPixelSize: 96) == nil)
}

@Test func cargaRealUsaAMesmaSondagemDoEngine() async throws {
    let path = try fixture("t96_24.flac").path
    let info = await loadQueueEntry(path: path)
    #expect(info.quality == "24/96")
    #expect(info.qualityDetail == "96 kHz · 24 bit · flac")
    #expect(info.duration == "0:06")
    #expect(info.problem == nil)
}

@Test func cargaRealComCapaTrazTagsEMiniatura() async throws {
    let info = await loadQueueEntry(path: try fixture("meta_fixture.flac").path)
    #expect(info.title == "Titulo Teste")
    #expect(info.subtitle == "Artista Teste — Album Teste")
    #expect(info.thumbnail != nil)
}

@Test func arquivoQueNaoAbreViraProblemaComAMensagemDoRust() async {
    let info = await loadQueueEntry(path: "/tmp/hog-audio-nao-existe-fila.flac")
    let problem = info.problem ?? ""
    #expect(!problem.isEmpty)
    // A mensagem do Rust, não a descrição do enum gerado pelo uniffi.
    #expect(!problem.hasPrefix("Load("))
    #expect(info.title == "hog-audio-nao-existe-fila")
}
