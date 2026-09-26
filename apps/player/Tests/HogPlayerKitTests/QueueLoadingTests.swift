import Foundation
import HogAudioBindings
@testable import HogPlayerKit
import Testing

/// Registra quem começou, em que ordem e quantos rodavam juntos — é o que prova o limite de
/// cargas simultâneas sem depender de relógio.
private actor RegistroDeCargas {
    private(set) var emAndamento = 0
    private(set) var pico = 0
    private(set) var ordem: [String] = []

    func comecou(_ path: String) {
        emAndamento += 1
        pico = max(pico, emAndamento)
        ordem.append(path)
    }

    func terminou() {
        emAndamento -= 1
    }
}

/// O atraso só existe para manter as cargas em voo ao mesmo tempo; o teste aguarda a task de
/// carga, nunca o relógio.
private func carregador(
    _ registro: RegistroDeCargas
) -> @Sendable (String) async -> QueueEntryInfo {
    { path in
        await registro.comecou(path)
        try? await Task.sleep(for: .milliseconds(20))
        await registro.terminou()
        return QueueEntryInfo(
            title: "tag de \(path)", artist: nil, album: nil, quality: "24/96",
            qualityDetail: nil, duration: "1:00", thumbnail: nil, problem: nil
        )
    }
}

@MainActor
private func modelo(
    com paths: [String],
    _ registro: RegistroDeCargas
) -> (PlayerViewModel, FakePlayer) {
    let fake = FakePlayer()
    let model = PlayerViewModel(player: fake, loadEntry: carregador(registro))
    fake.itens = paths.map { QueueItem(path: $0, failure: nil) }
    fake.indiceAtual = 0
    fake.estado = .loaded
    fake.versaoFila += 1
    model.refresh()
    return (model, fake)
}

@Test @MainActor func cadaFaixaQueEntraNaFilaGanhaSuaLinha() async {
    let registro = RegistroDeCargas()
    let paths = ["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"]
    let (model, _) = modelo(com: paths, registro)

    await model.entryLoading?.value

    for path in paths {
        #expect(model.entries[path]?.title == "tag de \(path)")
    }
}

@Test @MainActor func nuncaPassaDeQuatroCargasAoMesmoTempo() async {
    let registro = RegistroDeCargas()
    let paths = (0..<12).map { "/tmp/faixa-\($0).flac" }
    let (model, _) = modelo(com: paths, registro)

    await model.entryLoading?.value

    #expect(await registro.pico == 4)
    #expect(model.entries.count == 12)
}

@Test @MainActor func oTopoDaFilaCarregaPrimeiro() async {
    let registro = RegistroDeCargas()
    let paths = (0..<8).map { "/tmp/faixa-\($0).flac" }
    let (model, _) = modelo(com: paths, registro)

    await model.entryLoading?.value

    // Dentro de um lote de quatro a ordem de partida não é determinística; entre lotes é.
    let ordem = await registro.ordem
    #expect(Set(ordem.prefix(4)) == Set(paths.prefix(4)))
    #expect(Set(ordem.suffix(4)) == Set(paths.suffix(4)))
}

@Test @MainActor func refreshSemMudancaNaFilaNaoRecarrega() async {
    let registro = RegistroDeCargas()
    let (model, _) = modelo(com: ["/tmp/a.flac", "/tmp/b.flac"], registro)
    await model.entryLoading?.value

    model.refresh()
    model.refresh()
    await model.entryLoading?.value

    #expect(await registro.ordem.count == 2)
}

@Test @MainActor func faixaNovaCarregaSoElaMesma() async {
    let registro = RegistroDeCargas()
    let (model, fake) = modelo(com: ["/tmp/a.flac"], registro)
    await model.entryLoading?.value

    fake.itens.append(QueueItem(path: "/tmp/b.flac", failure: nil))
    fake.versaoFila += 1
    model.refresh()
    await model.entryLoading?.value

    #expect(await registro.ordem == ["/tmp/a.flac", "/tmp/b.flac"])
    #expect(model.entries.count == 2)
}

@Test @MainActor func limparAFilaDescartaAsLinhas() async {
    let registro = RegistroDeCargas()
    let (model, _) = modelo(com: ["/tmp/a.flac", "/tmp/b.flac"], registro)
    await model.entryLoading?.value
    #expect(model.entries.count == 2)

    model.clearQueue()
    await model.pendingCommand?.value

    #expect(model.entries.isEmpty)
}
