import Testing
import Foundation
@testable import HogPlayerKit
import HogAudioBindings

/// Dublê que registra o que foi chamado. Permite testar a lógica de comando sem tomar o
/// device de áudio da máquina onde os testes rodam.
final class FakePlayer: HogPlayerProtocol, @unchecked Sendable {
    var chamadas: [String] = []
    var estado: PlayerState = .idle
    var volumeAplicado: Float?

    func load(path: String) throws -> TrackFormat {
        chamadas.append("load")
        estado = .loaded
        return TrackFormat(sampleRate: 44100, bitDepth: 16, channels: 2,
                           codec: "flac", deviceName: "Fake", totalSeconds: 10)
    }
    func play() throws { chamadas.append("play"); estado = .playing }
    func pause() throws { chamadas.append("pause"); estado = .paused }
    func setVolume(scalar: Float) throws { volumeAplicado = scalar }
    func snapshot() -> Snapshot {
        Snapshot(state: estado, elapsedSeconds: 0, totalSeconds: 10,
                 underruns: 0, volumeScalar: 0.5)
    }
    func shutdown() { chamadas.append("shutdown") }
}

@Test @MainActor func oBotaoTocaQuandoParadoEPausaQuandoTocando() async {
    let fake = FakePlayer()
    let model = PlayerViewModel(player: fake)

    fake.estado = .loaded
    model.refresh()
    model.toggle()
    // O comando roda numa task de fundo — aguarda ela terminar em vez de adivinhar por tempo.
    await model.pendingCommand?.value
    #expect(fake.chamadas.contains("play"))

    fake.estado = .playing
    model.refresh()
    model.toggle()
    await model.pendingCommand?.value
    #expect(fake.chamadas.contains("pause"))
}

@Test @MainActor func semFaixaCarregadaOBotaoNaoFazNada() {
    let fake = FakePlayer()
    let model = PlayerViewModel(player: fake)
    model.refresh()
    model.toggle()
    #expect(fake.chamadas.isEmpty)
    // `pendingCommand` é atribuído de forma síncrona dentro de `toggle()` — se o guard fosse
    // removido, `chamadas` continuaria vazio no instante desta asserção (a task só roda depois
    // que esta função síncrona devolve o controle), mas `pendingCommand` já não seria nil.
    // É essa asserção que distingue guard presente de guard removido.
    #expect(model.pendingCommand == nil)
}

@Test @MainActor func oVolumeVaiDeZeroAUmParaOEngine() {
    let fake = FakePlayer()
    let model = PlayerViewModel(player: fake)
    model.applyVolume(0.35)
    #expect(fake.volumeAplicado == 0.35)
}

@Test @MainActor func oRefreshAtualizaOQueATelaMostra() {
    let fake = FakePlayer()
    fake.estado = .playing
    let model = PlayerViewModel(player: fake)
    model.refresh()
    #expect(model.display.isPlaying)
    #expect(model.display.canPause)
}
