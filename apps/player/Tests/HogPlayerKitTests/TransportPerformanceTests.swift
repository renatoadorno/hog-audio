import Combine
import Foundation
import HogAudioBindings
@testable import HogPlayerKit
import Testing

/// Segura uma carga de metadados até o teste mandar soltar — é o que prova que o transporte
/// não espera por ela, sem depender de relógio.
private actor Portao {
    private var aberto = false
    private var esperando: [CheckedContinuation<Void, Never>] = []

    func esperar() async {
        if aberto { return }
        await withCheckedContinuation { esperando.append($0) }
    }

    func abrir() {
        aberto = true
        for continuacao in esperando {
            continuacao.resume()
        }
        esperando.removeAll()
    }
}

@MainActor
private final class Termino {
    var aconteceu = false
}

/// Espera a task com prazo. `Task.value` não obedece cancelamento, e nem o `.timeLimit` do
/// Swift Testing solta um teste preso nele: se o comando voltar a esperar os metadados, sem
/// isto a suíte trava em vez de reprovar.
@MainActor
private func terminaEm(_ prazo: Duration, _ task: Task<Void, Never>?) async -> Bool {
    guard let task else { return true }
    let termino = Termino()
    Task { @MainActor in
        await task.value
        termino.aconteceu = true
    }
    let limite = ContinuousClock.now + prazo
    while !termino.aconteceu, ContinuousClock.now < limite {
        try? await Task.sleep(for: .milliseconds(10))
    }
    return termino.aconteceu
}

private func metadados(_ url: URL) -> TrackMetadata {
    TrackMetadata(title: "meta \(url.path)", artist: nil, album: nil, artwork: nil)
}

@MainActor
private func fila(
    _ quantas: Int,
    metadataLoader: @escaping @Sendable (URL) async -> TrackMetadata
) -> (PlayerViewModel, FakePlayer) {
    let fake = FakePlayer()
    fake.itens = (0..<quantas).map { QueueItem(path: "/tmp/\($0).flac", failure: nil) }
    fake.indiceAtual = 0
    fake.estado = .loaded
    fake.versaoFila = 1
    let model = PlayerViewModel(
        player: fake,
        loadEntry: { path in
            QueueEntryInfo(
                title: "fila \(path)", artist: "Artista", album: "Álbum", quality: nil,
                qualityDetail: nil, duration: nil, thumbnail: nil, problem: nil
            )
        },
        metadataLoader: metadataLoader
    )
    return (model, fake)
}

@Test @MainActor func nextNaoEsperaOsMetadados() async {
    let portao = Portao()
    let (model, fake) = fila(3) { url in
        await portao.esperar()
        return metadados(url)
    }

    model.next()
    let terminou = await terminaEm(.seconds(2), model.pendingCommand)
    #expect(terminou, "o comando ficou esperando a leitura de metadados")
    #expect(fake.navegacoes == [1])
    #expect(model.currentIndex == 1)

    await portao.abrir()
    await model.pendingCommand?.value
    await model.metadataLoading?.value
    #expect(model.display.title == "meta /tmp/1.flac")
}

@Test @MainActor func oTituloVemDaFilaAntesDosMetadados() async {
    let portao = Portao()
    let (model, _) = fila(3) { url in
        await portao.esperar()
        return metadados(url)
    }
    await model.entryLoading?.value

    model.next()
    let terminou = await terminaEm(.seconds(2), model.pendingCommand)
    #expect(terminou, "o comando ficou esperando a leitura de metadados")

    #expect(model.display.title == "fila /tmp/1.flac")
    #expect(model.display.artist == "Artista")
    await portao.abrir()
    await model.pendingCommand?.value
}

@Test @MainActor func metadadoAtrasadoDeFaixaAntigaEDescartado() async {
    let portaoDaFaixa1 = Portao()
    let (model, _) = fila(3) { url in
        if url.path == "/tmp/1.flac" {
            await portaoDaFaixa1.esperar()
        }
        return metadados(url)
    }

    model.next()
    let primeiroTerminou = await terminaEm(.seconds(2), model.pendingCommand)
    #expect(primeiroTerminou, "o comando ficou esperando a leitura de metadados")
    let cargaDaFaixa1 = model.metadataLoading
    model.next()
    let segundoTerminou = await terminaEm(.seconds(2), model.pendingCommand)
    #expect(segundoTerminou, "o comando ficou esperando a leitura de metadados")
    guard primeiroTerminou, segundoTerminou else {
        await portaoDaFaixa1.abrir()
        await model.pendingCommand?.value
        return
    }
    await model.metadataLoading?.value
    #expect(model.display.title == "meta /tmp/2.flac")

    // A faixa 1 termina de ler depois que a 2 já assumiu.
    await portaoDaFaixa1.abrir()
    await cargaDaFaixa1?.value
    model.refresh()
    #expect(model.display.title == "meta /tmp/2.flac")
}

@Test @MainActor func tresNextsRapidosViramUmaCargaSo() async {
    let (model, fake) = fila(5) { metadados($0) }

    model.next()
    model.next()
    model.next()
    await model.pendingCommand?.value

    #expect(fake.navegacoes == [3])
    #expect(model.currentIndex == 3)
}

@Test @MainActor func nextEPreviousSeguidosNaoCarregamNada() async {
    let (model, fake) = fila(5) { metadados($0) }

    model.next()
    model.previous()
    await model.pendingCommand?.value

    #expect(fake.navegacoes.isEmpty)
    #expect(model.currentIndex == 0)
}

@Test @MainActor func refreshSemMudancaNaoRepublicaATela() async {
    let (model, _) = fila(3) { metadados($0) }
    await model.entryLoading?.value
    model.refresh()

    final class Contador { var mudancas = 0 }
    let contador = Contador()
    let assinatura = model.objectWillChange.sink { _ in contador.mudancas += 1 }
    model.refresh()
    model.refresh()

    #expect(contador.mudancas == 0)
    assinatura.cancel()
}

@Test @MainActor func filaOcupadaNaoBloqueiaEEReliDepois() {
    let (model, fake) = fila(3) { metadados($0) }
    fake.itens.append(QueueItem(path: "/tmp/nova.flac", failure: nil))
    fake.versaoFila += 1

    fake.filaOcupada = true
    model.refresh()
    #expect(model.queue.count == 3)

    fake.filaOcupada = false
    model.refresh()
    #expect(model.queue.count == 4)
}
