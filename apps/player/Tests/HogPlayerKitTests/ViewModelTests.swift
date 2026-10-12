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
    var volumeReal: Float? = 0.5
    var mudo = false
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
    // Quantas vezes o volume chegou de fato ao "hardware". O slider emite um pedido por
    // quadro do arrasto; o que não pode acontecer é cada um deles virar uma escrita.
    var escritasDeVolume = 0
    var itens: [QueueItem] = []
    var indiceAtual: UInt32?
    var versaoFila: UInt64 = 0
    var leiturasDaFila = 0
    var avancos = 0
    var atrasoDeAvanco: TimeInterval = 0
    var atrasoDeSelecao: TimeInterval = 0
    // O relógio da faixa: é o que avança a cada tique do poll enquanto toca.
    var decorrido: Double = 0
    // A perda do device: o snapshot levanta a suspeita e o `recover` confirma, como no engine.
    var suspeitaDeDevice = false
    var seqProblema: UInt64 = 0
    var problema: String?
    var recuperacoes = 0
    // O aviso que o engine publica no fim de um Play, e um portão que segura o `play()` depois
    // de publicá-lo — é o que reproduz um tique do poll exibindo o aviso antes de o comando
    // voltar.
    var avisoAoTocar: String?
    var portaoDoPlay: DispatchSemaphore?

    private func track() -> TrackFormat {
        TrackFormat(sampleRate: 44100, bitDepth: 16, bitrateKbps: nil, channels: 2,
                    codec: "flac", deviceName: "Fake", totalSeconds: 10)
    }
    /// Como no engine, cada entrada nova ganha um id acima de todos os que já existem.
    private func novoItem(_ path: String) -> QueueItem {
        QueueItem(id: (itens.map(\.id).max() ?? 0) + 1, path: path, failure: nil)
    }
    /// Acrescenta sem mexer na versão da fila: o teste decide quando o model deve reler.
    func acrescentar(_ path: String) {
        lock.lock(); defer { lock.unlock() }
        itens.append(novoItem(path))
    }
    func appendTracks(paths: [String]) -> UInt32 {
        lock.lock(); defer { lock.unlock() }
        for path in paths { itens.append(novoItem(path)) }
        if indiceAtual == nil, !itens.isEmpty { indiceAtual = 0; estado = .loaded }
        if !paths.isEmpty { versaoFila += 1 }
        return UInt32(paths.count)
    }
    func queueItems() -> [QueueItem] {
        lock.lock(); defer { lock.unlock() }
        leiturasDaFila += 1
        return itens
    }
    func selectTrack(index: UInt32) throws -> TrackFormat {
        if atrasoDeSelecao > 0 { Thread.sleep(forTimeInterval: atrasoDeSelecao) }
        lock.lock(); defer { lock.unlock() }
        guard Int(index) < itens.count else { throw PlayerError.State(message: "índice inválido") }
        chamadas.append("select"); indiceAtual = index
        if estado != .playing { estado = .loaded }
        return track()
    }
    func nextTrack() throws -> TrackFormat {
        lock.lock(); defer { lock.unlock() }
        guard let current = indiceAtual, Int(current + 1) < itens.count else {
            throw PlayerError.State(message: "fim")
        }
        indiceAtual = current + 1
        return track()
    }
    func previousTrack() throws -> TrackFormat {
        lock.lock(); defer { lock.unlock() }
        guard let current = indiceAtual, current > 0 else {
            throw PlayerError.State(message: "início")
        }
        indiceAtual = current - 1
        return track()
    }
    func advance() throws -> TrackFormat? {
        if atrasoDeAvanco > 0 { Thread.sleep(forTimeInterval: atrasoDeAvanco) }
        lock.lock(); defer { lock.unlock() }
        avancos += 1
        guard let current = indiceAtual, Int(current + 1) < itens.count else { return nil }
        indiceAtual = current + 1; estado = .playing
        return track()
    }
    func selectTrackId(id: UInt64) throws -> TrackFormat {
        let index: Int? = {
            lock.lock(); defer { lock.unlock() }
            return itens.firstIndex { $0.id == id }
        }()
        guard let index else { throw PlayerError.State(message: "a faixa não está mais na fila") }
        return try selectTrack(index: UInt32(index))
    }
    /// Como o engine: removida a faixa atual, quem assume a posição é carregado aqui mesmo e
    /// o formato dela volta na resposta.
    func removeTrackId(id: UInt64) throws -> TrackFormat? {
        lock.lock(); defer { lock.unlock() }
        guard let index = itens.firstIndex(where: { $0.id == id }) else {
            throw PlayerError.State(message: "a faixa não está mais na fila")
        }
        chamadas.append("remove")
        let currentBefore = indiceAtual.map(Int.init)
        itens.remove(at: index); versaoFila += 1
        if currentBefore == index {
            guard index < itens.count else {
                indiceAtual = nil; estado = .idle
                return nil
            }
            indiceAtual = UInt32(index)
            return track()
        }
        if let currentBefore, index < currentBefore {
            indiceAtual = UInt32(currentBefore - 1)
        }
        return nil
    }
    func clearQueue() {
        lock.lock(); defer { lock.unlock() }
        itens.removeAll(); indiceAtual = nil; estado = .idle; versaoFila += 1
    }
    func play() throws {
        lock.lock()
        chamadas.append("play"); estado = .playing
        if let aviso = avisoAoTocar {
            problema = aviso
            seqProblema += 1
        }
        let portao = portaoDoPlay
        lock.unlock()
        portao?.wait()
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
        escritasDeVolume += 1
        if let falha = falhaAoAplicarVolume { throw falha }
        volumeAplicado = scalar
        volumeReal = scalar
    }
    func snapshot() -> Snapshot {
        lock.lock(); defer { lock.unlock() }
        return Snapshot(state: estado, elapsedSeconds: decorrido, totalSeconds: 10,
                         underruns: 0, volumeScalar: volumeReal, muted: mudo,
                         currentIndex: indiceAtual, queueLen: UInt32(itens.count),
                         queueVersion: versaoFila, deviceSuspect: suspeitaDeDevice,
                         problemSeq: seqProblema)
    }
    func recover() throws -> Bool {
        lock.lock(); defer { lock.unlock() }
        chamadas.append("recover")
        recuperacoes += 1
        guard suspeitaDeDevice else { return false }
        suspeitaDeDevice = false
        estado = .loaded
        problema = "o device de saída foi desconectado; a reprodução parou"
        seqProblema += 1
        return true
    }
    func lastProblem() -> String? {
        lock.lock(); defer { lock.unlock() }
        return problema
    }
    func shutdown() {
        lock.lock(); defer { lock.unlock() }
        chamadas.append("shutdown")
    }
    // Simula o engine ocupado numa troca de device: `tryQueueItems` desiste em vez de esperar.
    var filaOcupada = false
    var navegacoes: [Int32] = []

    func tryQueueItems() -> [QueueItem]? {
        lock.lock(); defer { lock.unlock() }
        guard !filaOcupada else { return nil }
        leiturasDaFila += 1
        return itens
    }
    func navigate(steps: Int32) throws -> TrackFormat {
        lock.lock(); defer { lock.unlock() }
        navegacoes.append(steps)
        guard let current = indiceAtual, !itens.isEmpty else {
            throw PlayerError.State(message: "sem faixa")
        }
        let alvo = min(max(Int(current) + Int(steps), 0), itens.count - 1)
        indiceAtual = UInt32(alvo)
        if estado != .playing { estado = .loaded }
        return track()
    }
    func lastTimings() -> TransitionTimings {
        TransitionTimings(openMs: 0, releaseMs: 0, reopenMs: 0, acquireMs: 0,
                          volumeMs: 0, prefillMs: 0, startMs: 0)
    }
}

/// Entradas com ids sequenciais a partir de 1, como o engine dá a uma fila nova.
func itens(_ paths: [String]) -> [QueueItem] {
    paths.enumerated().map { QueueItem(id: UInt64($0.offset + 1), path: $0.element, failure: nil) }
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

@Test @MainActor func semFaixaCarregadaOBotaoNaoFazNada() async {
    let fake = FakePlayer()
    let model = PlayerViewModel(player: fake)
    model.refresh()
    model.toggle()
    await model.pendingCommand?.value
    #expect(fake.chamadas.isEmpty)
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

@Test @MainActor func oArrastoDoSliderNaoViraUmaEscritaPorQuadro() async {
    let fake = FakePlayer()
    let model = PlayerViewModel(player: fake)

    // Um arrasto de slider emite um pedido por quadro. Encadear um comando por pedido faz a
    // fila drenar muito depois de o usuário soltar o controle: com 30 ms de espera por
    // escrita — o que o `write_volume_confirmed` do lado Rust cobrava —, 50 quadros viravam
    // 1,5 s de atraso. O que o hardware precisa é do último valor, não de todos eles.
    for i in 0..<50 {
        model.applyVolume(Double(i) / 50.0)
    }
    await model.pendingCommand?.value

    #expect(fake.escritasDeVolume < 50)
    // Coalescer não pode custar o destino: o valor que o usuário parou é o que vale.
    #expect(fake.volumeAplicado == Float(49.0 / 50.0))
    #expect(model.volume == 49.0 / 50.0)
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
    #expect(fake.volumeReal.map(Double.init) == model.volume)
}

@Test @MainActor func aFilaSoERelidaQuandoAVersaoMuda() {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac"])
    fake.indiceAtual = 0
    fake.versaoFila = 1
    fake.estado = .loaded
    let model = PlayerViewModel(player: fake)

    model.refresh()
    model.refresh()
    #expect(fake.leiturasDaFila == 1)

    fake.acrescentar("/tmp/b.flac")
    fake.versaoFila = 2
    model.refresh()
    #expect(fake.leiturasDaFila == 2)
    #expect(model.queue.count == 2)
}

@Test @MainActor func finishedDisparaUmUnicoAvancoEnquantoACargaCorre() async {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac", "/tmp/b.flac"])
    fake.indiceAtual = 0
    fake.versaoFila = 1
    fake.estado = .finished
    fake.atrasoDeAvanco = 0.15
    let model = PlayerViewModel(player: fake)

    model.refresh()
    let advance = model.pendingCommand
    for _ in 0..<5 { model.refresh() }
    await advance?.value

    #expect(fake.avancos == 1)
    #expect(model.currentIndex == 1)
}

@Test @MainActor func nextManualPendenteImpedeAutoAvancoDoMesmoFinished() async {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"])
    fake.indiceAtual = 0
    fake.versaoFila = 1
    fake.estado = .finished
    let model = PlayerViewModel(player: fake)

    model.next()
    let next = model.pendingCommand
    model.refresh()
    await next?.value

    #expect(fake.avancos == 0)
    #expect(model.currentIndex == 1)
}

@Test @MainActor func selecaoEPlayPreservamAOrdemDeChegada() async {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac", "/tmp/b.flac"])
    fake.indiceAtual = 0
    fake.versaoFila = 1
    fake.estado = .loaded
    fake.atrasoDeSelecao = 0.1
    let model = PlayerViewModel(player: fake)

    model.selectTrack(id: 2)
    model.toggle()
    await model.pendingCommand?.value

    #expect(fake.chamadas.suffix(2) == ["select", "play"])
    #expect(fake.indiceAtual == 1)
    #expect(fake.estado == .playing)
}

@Test @MainActor func shutdownCancelaTransporteQueAindaNaoChegouAoRust() async {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac", "/tmp/b.flac"])
    fake.indiceAtual = 0
    fake.versaoFila = 1
    fake.estado = .loaded
    let model = PlayerViewModel(player: fake)

    model.selectTrack(id: 2)
    let selection = model.pendingCommand
    model.shutdown()
    await selection?.value

    #expect(!fake.chamadas.contains("select"))
    #expect(fake.chamadas.contains("shutdown"))
    #expect(fake.indiceAtual == 0)
}

@Test @MainActor func remocaoClassificaFaixaAtualDepoisDaSelecaoPendente() async {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac", "/tmp/b.flac"])
    fake.indiceAtual = 0
    fake.versaoFila = 1
    fake.estado = .loaded
    fake.atrasoDeSelecao = 0.1
    let model = PlayerViewModel(player: fake)

    model.selectTrack(id: 2)
    model.removeTrack(id: 2)
    await model.pendingCommand?.value

    #expect(model.currentIndex == nil)
    #expect(model.display.title == "nenhuma faixa carregada")
    #expect(model.queue.map(\.path) == ["/tmp/a.flac"])
}

@Test @MainActor func duasRemocoesSeguidasApagamAsFaixasCertas() async {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"])
    fake.indiceAtual = 0
    fake.versaoFila = 1
    fake.estado = .loaded
    let model = PlayerViewModel(player: fake)

    // Dois Delete antes de o poll reler a fila: com índices, o segundo pedido chegaria com a
    // posição que a linha tinha antes da primeira remoção.
    model.removeTrack(id: 2)
    model.removeTrack(id: 3)
    await model.pendingCommand?.value

    #expect(fake.itens.map(\.path) == ["/tmp/a.flac"])
    #expect(model.errorMessage == nil)
}

@Test @MainActor func removerAFaixaAtualNaoCarregaDeNovoQuemAssumiu() async {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"])
    fake.indiceAtual = 0
    fake.versaoFila = 1
    fake.estado = .playing
    let model = PlayerViewModel(player: fake)

    model.removeTrack(id: 1)
    await model.pendingCommand?.value

    // O engine já carregou quem assumiu; carregar de novo soltaria e tomaria o device outra
    // vez, com um corte no som.
    #expect(!fake.chamadas.contains("select"))
    #expect(model.currentIndex == 0)
    #expect(model.currentItemID == 2)
    #expect(model.display.technicalLine.contains("44.1 kHz"))
}

@MainActor
private func tocandoComVolume(_ volume: Float?) -> (PlayerViewModel, FakePlayer) {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac", "/tmp/b.flac"])
    fake.indiceAtual = 0
    fake.versaoFila = 1
    fake.estado = .playing
    fake.volumeReal = volume
    let model = PlayerViewModel(player: fake)
    model.refresh()
    return (model, fake)
}

@Test @MainActor func oSliderSegueOVolumeMudadoPorFora() {
    let (model, fake) = tocandoComVolume(0.5)

    fake.volumeReal = 0.25
    model.refresh()

    #expect(model.volume == 0.25)
}

@Test @MainActor func oSliderNaoSegueOVolumeForaDeTocarOuPausar() {
    let (model, fake) = tocandoComVolume(0.5)
    fake.estado = .loaded

    fake.volumeReal = 0.25
    model.refresh()

    #expect(model.volume == 0.5)
}

@Test @MainActor func oSliderNaoPiscaNaTrocaDeFaixa() async {
    let (model, fake) = tocandoComVolume(0.5)
    fake.atrasoDeSelecao = 0.1

    // Enquanto a troca corre, o device pode relatar o volume de antes da aquisição.
    model.selectTrack(id: 2)
    fake.volumeReal = 0.9
    model.refresh()
    #expect(model.volume == 0.5)

    fake.volumeReal = 0.5
    await model.pendingCommand?.value
}

@Test @MainActor func oSliderNaoSobrescreveUmArrasto() {
    let (model, fake) = tocandoComVolume(0.5)

    model.setVolumeEditing(true)
    fake.volumeReal = 0.25
    model.refresh()
    #expect(model.volume == 0.5)

    model.setVolumeEditing(false)
    model.refresh()
    #expect(model.volume == 0.25)
}

@Test @MainActor func oSliderNaoVoltaEnquantoAEscritaNaoChegouAoDevice() async {
    let (model, fake) = tocandoComVolume(0.5)
    fake.escalarComAtraso = 0.25
    fake.atrasoDeVolume = 0.15

    model.applyVolume(0.25)
    let escrita = model.pendingCommand
    model.refresh()
    #expect(model.volume == 0.25)

    await escrita?.value
}

@Test @MainActor func deviceSemControleDeVolumeNaoMexeNoSlider() {
    let (model, fake) = tocandoComVolume(0.5)

    fake.volumeReal = nil
    model.refresh()

    #expect(model.volume == 0.5)
}

@Test @MainActor func volumeIgualAoDoDeviceNaoArredondaOSlider() async {
    let (model, _) = tocandoComVolume(0.5)

    // 0.35 em `Float` e de volta em `Double` vira 0.3499999…: o slider mostraria 34%.
    model.applyVolume(0.35)
    await model.pendingCommand?.value
    model.refresh()

    #expect(model.volume == 0.35)
}

@Test @MainActor func oMudoDoDeviceApareceNaTela() {
    let (model, fake) = tocandoComVolume(0.5)

    fake.mudo = true
    model.refresh()

    #expect(model.muted)
}

@Test @MainActor func problemaDoDeviceApareceUmaVezSo() async {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac"])
    fake.indiceAtual = 0
    fake.estado = .playing
    let model = PlayerViewModel(player: fake)

    fake.problema = "o device sumiu"
    fake.seqProblema = 1
    model.refresh()
    #expect(model.errorMessage == "o device sumiu")

    // Um comando bem-sucedido limpa a mensagem; o mesmo problema não pode voltar no tique
    // seguinte.
    model.applyVolume(0.3)
    await model.pendingCommand?.value
    #expect(model.errorMessage == nil)
    model.refresh()
    #expect(model.errorMessage == nil)
}

@Test @MainActor func avisoPublicadoNoPlayNaoSomeQuandoOPlayTermina() async {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac"])
    fake.indiceAtual = 0
    fake.estado = .loaded
    fake.avisoAoTocar = "o volume voltou ao teto"
    let portao = DispatchSemaphore(value: 0)
    fake.portaoDoPlay = portao
    let model = PlayerViewModel(player: fake)
    model.refresh()

    model.toggle()
    let limite = ContinuousClock.now + .seconds(2)
    while fake.snapshot().problemSeq == 0, ContinuousClock.now < limite {
        try? await Task.sleep(for: .milliseconds(1))
    }
    // O tique do poll mostra o aviso enquanto o Play ainda não voltou.
    model.refresh()
    #expect(model.errorMessage == "o volume voltou ao teto")

    portao.signal()
    await model.pendingCommand?.value
    #expect(model.errorMessage == "o volume voltou ao teto")
}

@Test @MainActor func problemaDeAntesDeAbrirNaoAparece() {
    let fake = FakePlayer()
    fake.problema = "de uma sessão anterior"
    fake.seqProblema = 3
    let model = PlayerViewModel(player: fake)

    model.refresh()

    #expect(model.errorMessage == nil)
}

@Test @MainActor func deviceSuspeitoNoFimDaFaixaRecuperaENaoAvanca() async {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac", "/tmp/b.flac"])
    fake.indiceAtual = 0
    fake.versaoFila = 1
    fake.estado = .finished
    fake.suspeitaDeDevice = true
    let model = PlayerViewModel(player: fake)

    model.refresh()
    await model.pendingCommand?.value
    model.refresh()

    // O auto-avanço tocaria sozinho no device que o macOS escolhesse no lugar do perdido.
    #expect(fake.recuperacoes == 1)
    #expect(fake.avancos == 0)
    #expect(fake.estado == .loaded)
    #expect(model.errorMessage?.contains("desconectado") == true)
}

@Test @MainActor func suspeitaRepetidaNoPollNaoEnfileiraOutraRecuperacao() async {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac"])
    fake.indiceAtual = 0
    fake.estado = .playing
    fake.suspeitaDeDevice = true
    let model = PlayerViewModel(player: fake)

    model.refresh()
    model.refresh()
    model.refresh()
    await model.pendingCommand?.value

    #expect(fake.recuperacoes == 1)
}

@Test @MainActor func doisTogglesRapidosTocamEDepoisPausam() async {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac"])
    fake.indiceAtual = 0
    fake.versaoFila = 1
    fake.estado = .loaded
    let model = PlayerViewModel(player: fake)

    model.toggle()
    model.toggle()
    await model.pendingCommand?.value

    #expect(fake.chamadas.suffix(2) == ["play", "pause"])
    #expect(fake.estado == .paused)
}
