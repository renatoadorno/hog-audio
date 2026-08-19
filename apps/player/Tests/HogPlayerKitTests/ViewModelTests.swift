import Testing
import Foundation
@testable import HogPlayerKit
import HogAudioBindings

/// Dublê que registra o que foi chamado. Permite testar a lógica de comando sem tomar o
/// device de áudio da máquina onde os testes rodam.
///
/// Desde que `toggle()`, `open()` e `applyVolume()` passaram a rodar o trabalho bloqueante
/// fora da main actor, mais de um `Task.detached` pode chamar este dublê ao mesmo tempo (é
/// exatamente o que o teste de corrida de metadados provoca de propósito) — daí o `lock`
/// em vez de confiar apenas no `@unchecked Sendable`.
final class FakePlayer: HogPlayerProtocol, @unchecked Sendable {
    private let lock = NSLock()
    var chamadas: [String] = []
    var estado: PlayerState = .idle
    var volumeAplicado: Float?
    // Volume que o device "de verdade" tem — o que `snapshot()` devolve. Só diverge de
    // `volumeAplicado` quando `falhaAoAplicarVolume` está setado, simulando uma recusa do
    // hardware.
    var volumeReal: Float = 0.5
    var falhaAoAplicarVolume: Error?
    // Simulam o `load` bloqueante do lado Rust (`stop_and_release` pode levar até 2 s) para
    // testes de corrida. O atraso é amarrado ao `path` pedido, não a quando o teste o seta —
    // a ordem real de chegada das duas `Task.detached` não é determinística.
    var atrasoDeCarga: TimeInterval = 0
    var caminhoComAtraso: String?
    // Mesma ideia para `setVolume`: o atraso é amarrado ao escalar pedido (capturado por
    // closure no instante do `applyVolume()`), não a um contador de chamadas — identifica a
    // requisição certa independente de qual `Task.detached` chega primeiro no dublê. 0.25 e
    // 0.75 são frações binárias exatas: `Float` e `Double` concordam bit a bit, então a
    // comparação abaixo não sofre de arredondamento.
    var atrasoDeVolume: TimeInterval = 0
    var escalarComAtraso: Float?

    func load(path: String) throws -> TrackFormat {
        if path == caminhoComAtraso, atrasoDeCarga > 0 {
            Thread.sleep(forTimeInterval: atrasoDeCarga)
        }
        lock.lock(); defer { lock.unlock() }
        chamadas.append("load")
        estado = .loaded
        return TrackFormat(sampleRate: 44100, bitDepth: 16, channels: 2,
                           codec: "flac", deviceName: "Fake", totalSeconds: 10)
    }
    func play() throws {
        lock.lock(); defer { lock.unlock() }
        chamadas.append("play"); estado = .playing
    }
    func pause() throws {
        lock.lock(); defer { lock.unlock() }
        chamadas.append("pause"); estado = .paused
    }
    func setVolume(scalar: Float) throws {
        if scalar == escalarComAtraso, atrasoDeVolume > 0 {
            Thread.sleep(forTimeInterval: atrasoDeVolume)
        }
        lock.lock(); defer { lock.unlock() }
        if let falha = falhaAoAplicarVolume { throw falha }
        volumeAplicado = scalar
        volumeReal = scalar
    }
    func snapshot() -> Snapshot {
        lock.lock(); defer { lock.unlock() }
        return Snapshot(state: estado, elapsedSeconds: 0, totalSeconds: 10,
                         underruns: 0, volumeScalar: volumeReal)
    }
    func shutdown() {
        lock.lock(); defer { lock.unlock() }
        chamadas.append("shutdown")
    }
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

@Test @MainActor func oVolumeVaiDeZeroAUmParaOEngine() async {
    let fake = FakePlayer()
    let model = PlayerViewModel(player: fake)
    model.applyVolume(0.35)
    // `setVolume` roda fora da main actor — aguarda o comando terminar em vez de adivinhar.
    await model.pendingCommand?.value
    #expect(fake.volumeAplicado == 0.35)
}

@Test @MainActor func volumeQueFalhaVoltaAoValorDoDeviceENaoAoPedido() async {
    let fake = FakePlayer()
    fake.volumeReal = 0.5
    fake.falhaAoAplicarVolume = PlayerError.Device(message: "device recusou o volume")
    let model = PlayerViewModel(player: fake)

    model.applyVolume(0.9)
    await model.pendingCommand?.value

    #expect(model.errorMessage != nil)
    // O slider não pode continuar exibindo 0.9: o device ficou em 0.5, e é isso que o
    // snapshot — a fonte confiável — reporta.
    #expect(model.volume == 0.5)
}

@Test @MainActor func chamadasConsecutivasDeVolumeAplicamAUltimaPedida() async {
    let fake = FakePlayer()
    let model = PlayerViewModel(player: fake)

    // A primeira chamada "demora" a chegar ao hardware (como um `setVolume` real disputando o
    // mutex que um `play()` segura na aquisição do hog); a segunda é instantânea. Sem
    // encadear `pendingCommand`, a segunda venceria a corrida e o valor antigo chegaria por
    // cima logo depois, em silêncio — exatamente o defeito que o encadeamento fecha.
    fake.escalarComAtraso = 0.25
    fake.atrasoDeVolume = 0.15

    model.applyVolume(0.25)
    let primeira = model.pendingCommand

    model.applyVolume(0.75)
    let segunda = model.pendingCommand

    await segunda?.value
    await primeira?.value

    #expect(fake.volumeAplicado == 0.75)
}

@Test @MainActor func erroDeVolumeSomeDepoisDeUmComandoBemSucedido() async {
    let fake = FakePlayer()
    fake.falhaAoAplicarVolume = PlayerError.Device(message: "device recusou o volume")
    let model = PlayerViewModel(player: fake)

    model.applyVolume(0.9)
    await model.pendingCommand?.value
    #expect(model.errorMessage != nil)

    // Sem os `errorMessage = nil` do caminho de sucesso, um erro antigo ficaria preso na
    // tela para sempre, mesmo depois de um comando que funcionou.
    fake.falhaAoAplicarVolume = nil
    model.applyVolume(0.4)
    await model.pendingCommand?.value

    #expect(model.errorMessage == nil)
}

@Test @MainActor func oRefreshAtualizaOQueATelaMostra() {
    let fake = FakePlayer()
    // O dublê começa em `.idle` de propósito: se `refresh()` fosse removido do teste, o
    // `display` calculado no `init` continuaria mostrando idle, e as asserções abaixo
    // reprovariam — é isso que torna a chamada a `refresh()` decisiva, não decorativa.
    let model = PlayerViewModel(player: fake)
    fake.estado = .playing
    model.refresh()
    #expect(model.display.isPlaying)
    #expect(model.display.canPause)
}

@Test @MainActor func oToggleReleOVolumeRealDoDeviceAoFimDoComando() async {
    let fake = FakePlayer()
    fake.estado = .loaded
    // Diferente do default de `volume` (0.5) e do default de `FakePlayer.volumeReal` (0.5):
    // se o teste usasse 0.5, um `toggle()` que não relê nada ainda passaria por acidente.
    fake.volumeReal = 0.82
    let model = PlayerViewModel(player: fake)
    model.refresh()

    model.toggle()
    await model.pendingCommand?.value

    // Comparado ao `Float` original, não ao literal `0.82`: a conversão Float → Double não
    // preserva os mesmos bits, e uma igualdade contra o literal reprovaria por
    // arredondamento, não pela ausência da sincronização que o teste quer provar.
    #expect(model.volume == Double(fake.volumeReal))
}

@Test @MainActor func aberturaAntigaNaoSobrescreveAAberturaMaisNova() async {
    let fake = FakePlayer()
    let model = PlayerViewModel(player: fake)

    let urlA = URL(fileURLWithPath: "/tmp/faixa-a-inexistente-hog-audio.flac")
    let urlB = URL(fileURLWithPath: "/tmp/faixa-b-inexistente-hog-audio.flac")
    // A primeira abertura demora para "carregar" (como o `load` real, que pode esperar até
    // 2 s o hardware trocar de rate); a segunda é instantânea e deve vencer. O atraso é
    // amarrado ao `path` de A, não a um instante do teste — a ordem real de chegada das duas
    // tasks não é determinística.
    fake.caminhoComAtraso = urlA.path
    fake.atrasoDeCarga = 0.15

    model.open(url: urlA)
    let primeira = model.pendingCommand

    model.open(url: urlB)
    let segunda = model.pendingCommand

    await segunda?.value
    await primeira?.value

    #expect(model.display.title == "faixa-b-inexistente-hog-audio")
}
