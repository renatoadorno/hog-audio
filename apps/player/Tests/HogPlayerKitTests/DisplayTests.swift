import Testing
import Foundation
@testable import HogPlayerKit
import HogAudioBindings

@Test func formataSegundosComoRelogio() {
    #expect(formatTime(0) == "0:00")
    #expect(formatTime(7) == "0:07")
    #expect(formatTime(83) == "1:23")
    #expect(formatTime(3599) == "59:59")
    #expect(formatTime(3600) == "1:00:00")
    #expect(formatTime(3723) == "1:02:03")
}

@Test func tempoInvalidoNaoQuebraORelogio() {
    // total_seconds vem zerado antes de qualquer carga, e NaN é o que sai de uma divisão
    // por taxa zero. Nenhum dos dois pode virar "-1:-1" na tela.
    #expect(formatTime(-5) == "0:00")
    #expect(formatTime(.nan) == "0:00")
    #expect(formatTime(.infinity) == "0:00")
}

private let faixa = TrackFormat(
    sampleRate: 96000, bitDepth: 24, channels: 2,
    codec: "flac", deviceName: "Fones de Ouvido Externos", totalSeconds: 208.7
)

private let tags = TrackMetadata(
    title: "House of Memories", artist: "Panic! At The Disco",
    album: "Death of a Bachelor", artwork: nil
)

private func snap(_ state: PlayerState, elapsed: Double = 0, underruns: UInt64 = 0) -> Snapshot {
    Snapshot(state: state, elapsedSeconds: elapsed, totalSeconds: 208.7,
             underruns: underruns, volumeScalar: 0.5)
}

@Test func tocandoMostraPausarEOProgresso() {
    let d = displayState(snapshot: snap(.playing, elapsed: 83), metadata: tags, format: faixa)
    #expect(d.isPlaying)
    #expect(d.canPause)
    #expect(d.elapsed == "1:23")
    #expect(d.total == "3:28")
    #expect(abs(d.progress - 83 / 208.7) < 0.001)
    #expect(d.title == "House of Memories")
}

@Test func semFaixaCarregadaNaoDaParaTocar() {
    let vazio = Snapshot(state: .idle, elapsedSeconds: 0, totalSeconds: 0,
                         underruns: 0, volumeScalar: 0.5)
    let d = displayState(snapshot: vazio, metadata: nil, format: nil)
    #expect(!d.canPlay)
    #expect(!d.canPause)
    #expect(d.progress == 0)
    #expect(d.technicalLine.isEmpty)
}

@Test func aLinhaTecnicaMostraOQueOProjetoFazDeDiferente() {
    let d = displayState(snapshot: snap(.playing), metadata: tags, format: faixa)
    #expect(d.technicalLine.contains("96 kHz"))
    #expect(d.technicalLine.contains("24 bit"))
    #expect(d.technicalLine.contains("flac"))
    #expect(d.technicalLine.contains("hog ativo"))
}

@Test func pausadoAindaSegurraODevice() {
    let d = displayState(snapshot: snap(.paused, elapsed: 40), metadata: tags, format: faixa)
    #expect(!d.isPlaying)
    #expect(d.canPlay)
    #expect(d.technicalLine.contains("hog ativo"))
}

@Test func carregadoAindaNaoSegurraODevice() {
    let d = displayState(snapshot: snap(.loaded), metadata: tags, format: faixa)
    #expect(d.canPlay)
    #expect(!d.technicalLine.contains("hog ativo"))
}

@Test func underrunsAparecemNaLinhaTecnica() {
    let d = displayState(snapshot: snap(.playing, underruns: 3), metadata: tags, format: faixa)
    #expect(d.technicalLine.contains("3 underruns"))
}

@Test func aFaixaTerminadaPodeSerTocadaDeNovo() {
    let d = displayState(snapshot: snap(.finished, elapsed: 208.7), metadata: tags, format: faixa)
    #expect(d.canPlay)
    #expect(!d.canPause)
}
