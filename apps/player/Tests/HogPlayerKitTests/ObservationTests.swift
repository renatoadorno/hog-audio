import Foundation
import HogAudioBindings
@testable import HogPlayerKit
import Observation
import Testing

/// `withObservationTracking` chama o `onChange` na thread de quem muda o valor e exige um
/// closure `@Sendable`. Aqui o model só muda na main actor, a mesma dos testes.
private final class Disparo: @unchecked Sendable {
    var aconteceu = false
}

/// Diz se `agir` notificou quem leu, antes, o que `ler` lê. É o que a tela sente: um body só
/// é recalculado quando algo que ele leu notifica.
@MainActor
private func notifica(lendo ler: () -> Void, quando agir: () -> Void) -> Bool {
    let disparo = Disparo()
    withObservationTracking(ler, onChange: { disparo.aconteceu = true })
    agir()
    return disparo.aconteceu
}

@MainActor
private func tocando() async -> (PlayerViewModel, FakePlayer) {
    let fake = FakePlayer()
    fake.itens = itens(["/tmp/a.flac", "/tmp/b.flac"])
    fake.indiceAtual = 0
    fake.versaoFila = 1
    fake.estado = .playing
    fake.decorrido = 1.0
    let model = PlayerViewModel(
        player: fake,
        loadEntry: { path in
            QueueEntryInfo(
                title: "fila \(path)", artist: nil, album: nil, quality: nil,
                qualityDetail: nil, duration: nil, thumbnail: nil, problem: nil
            )
        },
        metadataLoader: { url in
            TrackMetadata(title: url.path, artist: nil, album: nil, artwork: nil)
        }
    )
    await model.entryLoading?.value
    model.refresh()
    return (model, fake)
}

@Test @MainActor func mudancaDeEstadoNotificaQuemLeATela() async {
    let (model, fake) = await tocando()

    let notificou = notifica(lendo: { _ = model.display }) {
        fake.estado = .paused
        model.refresh()
    }

    #expect(notificou)
}

/// Tocando, o relógio muda a cada tique do poll, dez vezes por segundo. Só o relógio pode
/// ser recalculado: a capa, o título e a fila ficam como estão. O tique cruza o segundo, para
/// mudar tanto o texto do tempo quanto a fração tocada.
@Test @MainActor func tiqueDoRelogioNaoNotificaQuemLeATelaOuAFila() async {
    let (model, fake) = await tocando()

    let notificou = notifica(lendo: {
        _ = model.display
        _ = model.queue
        _ = model.currentIndex
        _ = model.entries
    }) {
        fake.decorrido = 2.0
        model.refresh()
    }

    #expect(!notificou)
}

@Test @MainActor func tiqueDoRelogioNotificaQuemLeORelogio() async {
    let (model, fake) = await tocando()

    let notificou = notifica(lendo: { _ = model.clock }) {
        fake.decorrido = 2.0
        model.refresh()
    }

    #expect(notificou)
}

@Test @MainActor func refreshSemMudancaNaoNotificaNinguem() async {
    let (model, _) = await tocando()

    let notificou = notifica(lendo: {
        _ = model.display
        _ = model.clock
        _ = model.queue
        _ = model.currentIndex
        _ = model.entries
        _ = model.errorMessage
        _ = model.volume
    }) {
        model.refresh()
        model.refresh()
    }

    #expect(!notificou)
}
