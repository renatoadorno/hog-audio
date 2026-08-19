import Foundation
import HogAudioBindings
import os

// O protocolo que o ViewModel consome é o `HogPlayerProtocol` **gerado pelo uniffi** — ele já
// declara exatamente os seis métodos e já é `Sendable`. Escrever um protocolo próprio aqui
// duplicaria a fronteira e sairia de sincronia no dia em que a API do Rust mudasse.

private let logger = Logger(subsystem: "local.hogaudio.player", category: "PlayerViewModel")

@MainActor
public final class PlayerViewModel: ObservableObject {
    @Published public private(set) var display: DisplayState
    @Published public var volume: Double = 0.5
    @Published public private(set) var errorMessage: String?

    /// A task do último comando emitido. Existe para que quem precise saber quando o comando
    /// terminou possa aguardá-la, em vez de adivinhar por tempo — e para que dois comandos
    /// consecutivos sejam observáveis em ordem.
    ///
    /// Guardar a task aqui não implica esperar por ela: cada método decide, no próprio corpo,
    /// se encadeia na `pendingCommand` anterior (preserva ordem de chegada ao hardware, ao
    /// custo de atraso) ou se sobrescreve (responde na hora, aceitando que o resultado antigo
    /// pode chegar depois — só é seguro quando algo mais garante que o atrasado não vence).
    /// Ver `applyVolume()`, `toggle()` e `open()` para o porquê de cada escolha.
    public private(set) var pendingCommand: Task<Void, Never>?

    private let player: any HogPlayerProtocol
    private var metadata: TrackMetadata?
    private var format: TrackFormat?
    private var timer: Timer?

    /// Incrementado a cada `open()`: a task de metadados de uma abertura mais antiga que
    /// aterrissa depois de uma mais nova não pode sobrescrever o que já está na tela.
    private var openGeneration = 0

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
        // Sem isto, um segundo `onAppear` vazaria o timer anterior — o run loop o retém, e
        // a taxa de poll dobraria para sempre.
        stopPolling()
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

    // `isolated`: `timer` é estado da main actor, e `Timer` não é `Sendable` — sem isolar o
    // deinit, o compilador reprova o acesso.
    isolated deinit {
        // A garantia não pode depender de a camada de interface lembrar de chamar
        // `stopPolling`/`shutdown` antes do descarte — sem isto o timer, retido pelo run
        // loop, dispara para sempre chamando um closure que não faz mais nada.
        timer?.invalidate()
    }

    public func refresh() {
        display = displayState(
            snapshot: player.snapshot(), metadata: metadata, format: format
        )
    }

    public func open(url: URL) {
        errorMessage = nil
        openGeneration += 1
        let generation = openGeneration
        let tags = Task { await loadMetadata(from: url) }

        // `load` pode fazer `stop_and_release`, que junta a thread produtora e espera o
        // hardware confirmar a troca de rate — até 2 s. Bloquear a main actor por isso
        // travaria a janela inteira; o trabalho roda fora dela, como em `toggle()`.
        //
        // Sobrescreve `pendingCommand` em vez de encadear no anterior — de propósito, ao
        // contrário de `applyVolume()`. Encadear faria uma segunda abertura esperar até 2 s
        // pela primeira antes sequer de começar a carregar, e essa espera não protegeria nada:
        // quem já resolve "que faixa mais nova sobrescreve mais antiga aqui" é o `guard
        // generation == openGeneration` abaixo, que descarta o resultado atrasado sem atrasar
        // o trabalho novo. Encadear seria pura perda de responsividade.
        pendingCommand = Task {
            do {
                let loaded = try await Task.detached { [player] in
                    try player.load(path: url.path)
                }.value
                // Uma abertura mais nova já pode ter começado enquanto esta esperava o
                // hardware — sem este cheque, ela sobrescreveria a faixa certa com a errada.
                guard generation == openGeneration else { return }
                format = loaded
                metadata = nil
                refresh()
            } catch {
                guard generation == openGeneration else { return }
                format = nil
                metadata = nil
                errorMessage = "\(error)"
                refresh()
                return
            }
            let meta = await tags.value
            guard generation == openGeneration else { return }
            metadata = meta
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
        //
        // Sobrescreve em vez de encadear — ao contrário de `applyVolume()` — porque `play()` e
        // `pause()` carregam guarda de estado do próprio lado Rust: um pedido que não faz
        // sentido na ordem em que chegou (`PlayFailure::State`, ver `rust/src/api.rs`) falha
        // alto, tipado, e aparece em `errorMessage`. `set_volume` não tem essa guarda — aceita
        // qualquer escalar em qualquer ordem, silenciosamente — e é isso que torna o
        // encadeamento indispensável lá e dispensável aqui.
        pendingCommand = Task {
            do {
                try await Task.detached { [player] in
                    if devePausar {
                        try player.pause()
                    } else {
                        try player.play()
                    }
                }.value
                // Limpa uma falha anterior: sem isto, um play() que falhou deixaria a
                // mensagem de erro presa na tela mesmo depois de um pause() bem-sucedido.
                errorMessage = nil
            } catch {
                errorMessage = "\(error)"
            }
            // Só o `play()` publica volume no `SharedStatus` do lado Rust — quando o teto de
            // segurança agiu, é aqui, ao fim do comando, que o slider precisa reler o valor
            // real do hardware.
            volume = Double(player.snapshot().volumeScalar)
            refresh()
        }
    }

    public func applyVolume(_ scalar: Double) {
        // Publica na hora: o slider não pode esperar o mutex do lado Rust — o mesmo que um
        // `play()` segura durante a aquisição inteira do hog — para responder ao arrasto.
        volume = scalar
        // O volume é a única coisa nesta interface que mexe em quanto sinal chega ao fone —
        // falhar calado aqui, como o shutdown, deixaria o usuário sem saber que o device não
        // mudou de verdade.
        //
        // O `Slider` chama isto continuamente durante o arrasto, não só ao soltar — tirar o
        // trabalho da main actor removeu de graça a serialização que ela dava, e nada a
        // substituiu: sem encadear, cada chamada dispara uma `Task.detached` independente, e
        // nada garante que cheguem ao hardware na ordem em que o usuário pediu (o mutex do
        // lado Rust serializa exclusão mútua, não ordem de chegada — ao contrário de
        // `play()`/`pause()`, `set_volume` não tem guarda de estado que reprove um pedido fora
        // de ordem). Encadear na `pendingCommand` anterior preserva a ordem sem voltar a
        // bloquear a main actor: a task só chama o engine depois que a anterior terminou.
        let previous = pendingCommand
        pendingCommand = Task {
            await previous?.value
            do {
                try await Task.detached { [player] in
                    try player.setVolume(scalar: Float(scalar))
                }.value
                errorMessage = nil
            } catch {
                // O valor publicado tem de dizer a verdade: se o device recusou, o slider não
                // pode continuar exibindo o pedido como se tivesse sido aceito. O snapshot é a
                // fonte confiável do volume real, mantida pelo engine — nada de cache próprio.
                volume = Double(player.snapshot().volumeScalar)
                errorMessage = "\(error)"
            }
        }
    }

    public func shutdown() {
        stopPolling()
        // `shutdown()` lança justamente para sinalizar quando o device não volta ao estado
        // original — engolir em silêncio deixaria o Mac com outro sample rate sem aviso. A
        // janela que exibiria `errorMessage` já está sendo destruída neste instante: o log é
        // o único canal que sobrevive ao processo para registrar o que aconteceu.
        do {
            try player.shutdown()
        } catch {
            errorMessage = "\(error)"
            logger.error("falha ao encerrar o player: \(String(describing: error), privacy: .public)")
        }
    }
}
