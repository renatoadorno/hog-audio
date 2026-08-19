import Foundation
import HogAudioBindings

// O protocolo que o ViewModel consome é o `HogPlayerProtocol` **gerado pelo uniffi** — ele já
// declara exatamente os seis métodos e já é `Sendable`. Escrever um protocolo próprio aqui
// duplicaria a fronteira e sairia de sincronia no dia em que a API do Rust mudasse.

@MainActor
public final class PlayerViewModel: ObservableObject {
    @Published public private(set) var display: DisplayState
    @Published public var volume: Double = 0.5
    @Published public private(set) var errorMessage: String?

    /// A task do último comando emitido. Existe para que quem precise saber quando o comando
    /// terminou possa aguardá-la, em vez de adivinhar por tempo — e para que dois comandos
    /// consecutivos sejam observáveis em ordem.
    public private(set) var pendingCommand: Task<Void, Never>?

    private let player: any HogPlayerProtocol
    private var metadata: TrackMetadata?
    private var format: TrackFormat?
    private var timer: Timer?

    public init(player: any HogPlayerProtocol) {
        self.player = player
        self.display = displayState(
            snapshot: player.snapshot(), metadata: nil, format: nil
        )
    }

    public convenience init() {
        self.init(player: HogPlayer())
    }

    public func startPolling() {
        // Dez vezes por segundo: suficiente para o relógio parecer contínuo, e barato porque
        // o snapshot só lê atômicos do lado Rust.
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.refresh() }
        }
    }

    public func stopPolling() {
        timer?.invalidate()
        timer = nil
    }

    public func refresh() {
        display = displayState(
            snapshot: player.snapshot(), metadata: metadata, format: format
        )
    }

    public func open(url: URL) {
        errorMessage = nil
        let tags = Task { await loadMetadata(from: url) }
        do {
            format = try player.load(path: url.path)
            metadata = nil
            Task { @MainActor in
                metadata = await tags.value
                refresh()
            }
            volume = Double(player.snapshot().volumeScalar)
            refresh()
        } catch {
            format = nil
            metadata = nil
            errorMessage = "\(error)"
            refresh()
        }
    }

    public func toggle() {
        let deveTocar = display.canPlay
        let devePausar = display.canPause
        guard deveTocar || devePausar else { return }

        // O trabalho bloqueante (adquirir o hog, trocar o rate, esperar o hardware) roda
        // isolado num `Task.detached`, que só captura `player` — `Sendable` pelo protocolo
        // gerado. A task externa não é detached: herda a main actor, então é o lugar seguro
        // para tocar `self` de volta ao fim, sem violar o checking estrito de Swift 6.
        // Guardada em `pendingCommand` para que o comando seja observável sem depender de
        // relógio: quem precisar saber quando terminou aguarda a task, não um `sleep`.
        pendingCommand = Task {
            do {
                try await Task.detached { [player] in
                    if devePausar {
                        try player.pause()
                    } else {
                        try player.play()
                    }
                }.value
            } catch {
                errorMessage = "\(error)"
            }
            refresh()
        }
    }

    public func applyVolume(_ scalar: Double) {
        volume = scalar
        try? player.setVolume(scalar: Float(scalar))
    }

    public func shutdown() {
        stopPolling()
        // `shutdown()` lança justamente para sinalizar quando o device não volta ao estado
        // original — engolir em silêncio deixaria o Mac com outro sample rate sem aviso.
        do {
            try player.shutdown()
        } catch {
            errorMessage = "\(error)"
        }
    }
}
