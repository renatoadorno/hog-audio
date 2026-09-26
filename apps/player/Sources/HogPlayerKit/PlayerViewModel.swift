import Foundation
import HogAudioBindings
import os

// O protocolo que o ViewModel consome é o `HogPlayerProtocol` **gerado pelo uniffi** — ele já
// declara exatamente a fronteira atual e já é `Sendable`. Escrever um protocolo próprio aqui
// duplicaria a fronteira e sairia de sincronia no dia em que a API do Rust mudasse.

private let logger = Logger(subsystem: "local.hogaudio.player", category: "PlayerViewModel")

private func milliseconds(_ duration: Duration) -> Double {
    let (seconds, attoseconds) = duration.components
    return Double(seconds) * 1000 + Double(attoseconds) / 1e15
}

@MainActor
public final class PlayerViewModel: ObservableObject {
    @Published public private(set) var display: DisplayState
    @Published public var volume: Double = 0.5
    @Published public private(set) var errorMessage: String?
    @Published public private(set) var queue: [QueueItem] = []
    @Published public private(set) var currentIndex: UInt32?
    /// Título, capa, qualidade e duração de cada faixa da fila, pelo caminho. Chega aos poucos
    /// depois que a faixa entra na fila; até lá a linha mostra o nome do arquivo.
    @Published public private(set) var entries: [String: QueueEntryInfo] = [:]

    /// A carga das linhas em andamento, exposta pelo mesmo motivo de `pendingCommand`: quem
    /// precisar saber quando terminou aguarda a task em vez de adivinhar por tempo.
    public private(set) var entryLoading: Task<Void, Never>?

    /// A task do último comando emitido. Existe para que quem precise saber quando o comando
    /// terminou possa aguardá-la, em vez de adivinhar por tempo — e para que dois comandos
    /// consecutivos sejam observáveis em ordem.
    ///
    /// Os comandos de transporte são encadeados por `serializedCommand`; o worker de volume
    /// pode rodar em paralelo porque coalesce pedidos e o mutex do Rust protege o hardware.
    /// `pendingCommand` continua expondo a task mais recente para testes e desligamento.
    public private(set) var pendingCommand: Task<Void, Never>?
    private var serializedCommand: Task<Void, Never>?

    private let player: any HogPlayerProtocol
    private let loadEntry: @Sendable (String) async -> QueueEntryInfo
    private var metadata: TrackMetadata?
    private var format: TrackFormat?
    private var timer: Timer?
    private var lastQueueVersion: UInt64?
    private var advancing = false
    private var advancedFinishedIndex: UInt32?
    private var transportCommandsInFlight = 0
    private var shuttingDown = false

    /// O volume que o usuário pediu por último e que ainda não foi ao hardware, e se já há um
    /// worker drenando. Ambos vivem na main actor, então o arrasto os atualiza sem trava.
    /// Ver `applyVolume()`.
    private var volumeDesejado: Double?
    private var escrevendoVolume = false

    /// Soltar uma pasta de mil faixas não pode disparar mil leituras de tag e decodificações
    /// de capa ao mesmo tempo: quatro mantêm a lista enchendo de cima para baixo sem disputar
    /// CPU e disco com a reprodução.
    private let maxConcurrentEntryLoads = 4
    private var pendingEntryPaths: [String] = []
    private var loadingEntryPaths: Set<String> = []
    private var queuedPaths: Set<String> = []

    /// Os metadados completos (com a capa em resolução cheia) da faixa atual carregam fora da
    /// cadeia de comandos: o próximo Next não espera por eles. `metadataPath` diz de qual
    /// faixa são os que estão em `metadata`.
    private let metadataLoader: @Sendable (URL) async -> TrackMetadata
    private var metadataPath: String?
    public private(set) var metadataLoading: Task<Void, Never>?

    /// A navegação que ainda espera o comando anterior terminar. Cliques novos somam nela em
    /// vez de enfileirar outra carga; `nil` quando nenhuma está esperando.
    private var pendingNavigation: NavigationRequest?

    public init(
        player: any HogPlayerProtocol,
        loadEntry: @escaping @Sendable (String) async -> QueueEntryInfo = loadQueueEntry(path:),
        metadataLoader: @escaping @Sendable (URL) async -> TrackMetadata = loadMetadata(from:)
    ) {
        self.player = player
        self.loadEntry = loadEntry
        self.metadataLoader = metadataLoader
        let snapshot = player.snapshot()
        self.display = displayState(snapshot: snapshot, metadata: nil, format: nil)
        self.currentIndex = snapshot.currentIndex
        self.queue = player.queueItems()
        self.lastQueueVersion = snapshot.queueVersion
        syncEntries()
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
        let timer = Timer(timeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.refresh() }
        }
        // No modo padrão o timer para enquanto o usuário arrasta o volume ou rola a fila: o
        // relógio congelava e o fim da faixa só era percebido quando ele soltava.
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
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
        let snapshot = player.snapshot()
        // Atribuir sem mudança republica e força a tela inteira a se recalcular, dez vezes
        // por segundo, mesmo com o player parado.
        if currentIndex != snapshot.currentIndex {
            currentIndex = snapshot.currentIndex
        }
        // `tryQueueItems` nunca espera o engine: com um comando segurando o lock (troca de
        // device leva segundos), a versão não avança e o próximo tique tenta de novo.
        if lastQueueVersion != snapshot.queueVersion, let items = player.tryQueueItems() {
            queue = items
            lastQueueVersion = snapshot.queueVersion
            syncEntries()
        }
        let shown = displayState(
            snapshot: snapshot,
            metadata: shownMetadata(for: snapshot.currentIndex),
            format: format
        )
        if display != shown {
            display = shown
        }

        if snapshot.state != .finished {
            advancedFinishedIndex = nil
        }
        if !shuttingDown,
           snapshot.state == .finished,
           let current = snapshot.currentIndex,
           current + 1 < snapshot.queueLen,
           !advancing,
           transportCommandsInFlight == 0,
           advancedFinishedIndex != current {
            advanceFromFinished(index: current)
        }
    }

    public func open(url: URL) {
        add(urls: [url])
    }

    /// O que a tela mostra da faixa atual. Logo depois de uma troca os metadados completos
    /// ainda não chegaram: o título vem da linha da fila (ou do nome do arquivo), e a capa da
    /// faixa anterior fica na tela até a nova chegar, em vez de piscar o placeholder.
    private func shownMetadata(for index: UInt32?) -> TrackMetadata? {
        guard let index, Int(index) < queue.count else { return metadata }
        let path = queue[Int(index)].path
        if metadataPath == path { return metadata }
        let entry = entries[path]
        return TrackMetadata(
            title: entry?.title
                ?? URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent,
            artist: entry?.artist,
            album: entry?.album,
            artwork: metadata?.artwork
        )
    }

    private var currentPath: String? {
        guard let currentIndex, Int(currentIndex) < queue.count else { return nil }
        return queue[Int(currentIndex)].path
    }

    /// Alinha as linhas carregadas com a fila atual: descarta as de caminhos que saíram e
    /// enfileira, na ordem da fila, as que ainda não existem — o topo visível preenche primeiro.
    private func syncEntries() {
        queuedPaths = Set(queue.map(\.path))
        if entries.keys.contains(where: { !queuedPaths.contains($0) }) {
            entries = entries.filter { queuedPaths.contains($0.key) }
        }
        pendingEntryPaths.removeAll { !queuedPaths.contains($0) }

        var known = Set(entries.keys).union(loadingEntryPaths).union(pendingEntryPaths)
        for item in queue where !known.contains(item.path) {
            pendingEntryPaths.append(item.path)
            known.insert(item.path)
        }

        guard entryLoading == nil, !pendingEntryPaths.isEmpty, !shuttingDown else { return }
        entryLoading = Task {
            await drainEntryLoads()
            entryLoading = nil
        }
    }

    private func drainEntryLoads() async {
        await withTaskGroup(of: (String, QueueEntryInfo).self) { group in
            var running = 0
            while true {
                while running < maxConcurrentEntryLoads, !shuttingDown, !pendingEntryPaths.isEmpty {
                    let path = pendingEntryPaths.removeFirst()
                    loadingEntryPaths.insert(path)
                    group.addTask { [loadEntry] in (path, await loadEntry(path)) }
                    running += 1
                }
                guard let (path, info) = await group.next() else { return }
                running -= 1
                loadingEntryPaths.remove(path)
                // A faixa pode ter saído da fila enquanto carregava.
                if !shuttingDown, queuedPaths.contains(path) {
                    entries[path] = info
                }
            }
        }
    }

    public func add(urls: [URL]) {
        guard !shuttingDown else { return }
        let queuedAt = ContinuousClock.now
        let previous = serializedCommand
        transportCommandsInFlight += 1
        let task = Task {
            defer { transportCommandsInFlight -= 1 }
            await previous?.value
            guard !shuttingDown else { return }
            let wasEmpty = queue.isEmpty
            let files = await Task.detached { audioFiles(from: urls) }.value
            guard !files.isEmpty else {
                errorMessage = "nenhum arquivo de áudio encontrado"
                return
            }
            let paths = files.map(\.path)
            _ = await Task.detached { [player] in player.appendTracks(paths: paths) }.value
            refresh()
            if wasEmpty {
                // `appendTracks` mantém a assinatura simples da fronteira e não propaga a
                // falha da carga inicial. A seleção explícita torna esse erro visível.
                await loadSelection(index: 0, command: "carga inicial", queuedAt: queuedAt)
            }
        }
        serializedCommand = task
        pendingCommand = task
    }

    public func selectTrack(index: UInt32) {
        guard !shuttingDown else { return }
        let queuedAt = ContinuousClock.now
        let previous = serializedCommand
        transportCommandsInFlight += 1
        let task = Task {
            defer { transportCommandsInFlight -= 1 }
            await previous?.value
            guard !shuttingDown else { return }
            await loadSelection(index: index, command: "seleção", queuedAt: queuedAt)
        }
        serializedCommand = task
        pendingCommand = task
    }

    public func next() {
        navigate(by: 1)
    }

    public func previous() {
        navigate(by: -1)
    }

    /// Cinco cliques rápidos em Next eram cinco cargas completas, cada uma soltando e tomando
    /// o device. Enquanto a navegação ainda espera o comando anterior, cliques novos só somam
    /// passos nela; quando ela chega ao Rust, uma carga só leva à faixa final.
    private func navigate(by delta: Int32) {
        guard !shuttingDown else { return }
        if let pending = pendingNavigation {
            pending.steps += delta
            return
        }
        let request = NavigationRequest(steps: delta)
        pendingNavigation = request
        let queuedAt = ContinuousClock.now
        let previous = serializedCommand
        transportCommandsInFlight += 1
        let task = Task {
            defer { transportCommandsInFlight -= 1 }
            await previous?.value
            // Daqui em diante esta navegação vai ao Rust: um clique novo abre outra.
            if pendingNavigation === request {
                pendingNavigation = nil
            }
            guard !shuttingDown else { return }
            let steps = request.steps
            guard steps != 0 else { return }
            do {
                let loaded = try await timed("navegação \(steps)", queuedAt: queuedAt) {
                    [player] in try player.navigate(steps: steps)
                }
                apply(loaded: loaded)
            } catch {
                errorMessage = "\(error)"
                refresh()
            }
        }
        serializedCommand = task
        pendingCommand = task
    }

    public func removeTrack(index: UInt32) {
        guard !shuttingDown else { return }
        let queuedAt = ContinuousClock.now
        let previous = serializedCommand
        transportCommandsInFlight += 1
        let task = Task {
            defer { transportCommandsInFlight -= 1 }
            await previous?.value
            guard !shuttingDown else { return }
            let removedCurrent = currentIndex == index
            do {
                try await Task.detached { [player] in try player.removeTrack(index: index) }.value
                errorMessage = nil
                refresh()
                if removedCurrent, let currentIndex {
                    // `removeTrack` não devolve o formato de quem assumiu a posição.
                    await loadSelection(index: currentIndex, command: "remoção", queuedAt: queuedAt)
                } else if removedCurrent {
                    metadata = nil
                    metadataPath = nil
                    format = nil
                    refresh()
                }
            } catch {
                errorMessage = "\(error)"
                refresh()
            }
        }
        serializedCommand = task
        pendingCommand = task
    }

    public func clearQueue() {
        guard !shuttingDown else { return }
        let previous = serializedCommand
        transportCommandsInFlight += 1
        let task = Task {
            defer { transportCommandsInFlight -= 1 }
            await previous?.value
            guard !shuttingDown else { return }
            do {
                try await Task.detached { [player] in try player.clearQueue() }.value
                metadataLoading?.cancel()
                metadata = nil
                metadataPath = nil
                format = nil
                errorMessage = nil
            } catch {
                errorMessage = "\(error)"
            }
            refresh()
        }
        serializedCommand = task
        pendingCommand = task
    }

    private func loadSelection(
        index: UInt32,
        command: String,
        queuedAt: ContinuousClock.Instant
    ) async {
        do {
            let loaded = try await timed(command, queuedAt: queuedAt) { [player] in
                try player.selectTrack(index: index)
            }
            apply(loaded: loaded)
        } catch {
            errorMessage = "\(error)"
            refresh()
        }
    }

    /// Não espera os metadados: antes, o comando seguinte ficava atrás da leitura inteira da
    /// capa pelo AVFoundation. O título aparece na hora pela linha da fila
    /// (`shownMetadata`) e a capa chega quando a leitura terminar.
    private func apply(loaded: TrackFormat) {
        format = loaded
        errorMessage = nil
        refresh()
        reloadMetadataForCurrentTrack()
    }

    private func reloadMetadataForCurrentTrack() {
        metadataLoading?.cancel()
        guard let path = currentPath else {
            metadata = nil
            metadataPath = nil
            refresh()
            return
        }
        // Previous que rebobina recarrega a mesma faixa: os metadados já são dela.
        guard path != metadataPath else { return }
        let metadataLoader = self.metadataLoader
        metadataLoading = Task {
            let startedAt = ContinuousClock.now
            let loaded = await metadataLoader(URL(fileURLWithPath: path))
            // Outra faixa pode ter assumido enquanto esta lia: o resultado dela não vale mais.
            guard !Task.isCancelled, currentPath == path else { return }
            let elapsed = milliseconds(ContinuousClock.now - startedAt)
            logger.notice("metadados: \(String(format: "%.0f", elapsed), privacy: .public) ms")
            metadata = loaded
            metadataPath = path
            refresh()
        }
    }

    /// Roda uma chamada bloqueante ao Rust fora da main actor e registra quanto ela levou —
    /// a espera na fila de comandos, a chamada em si e as etapas que o engine mediu dentro
    /// dela. É o que permite comparar antes e depois com números do device real.
    private func timed<T: Sendable>(
        _ command: String,
        queuedAt: ContinuousClock.Instant,
        _ work: @escaping @Sendable () throws -> T
    ) async throws -> T {
        let startedAt = ContinuousClock.now
        defer { logTransition(command, queuedAt: queuedAt, startedAt: startedAt) }
        return try await Task.detached { try work() }.value
    }

    private func logTransition(
        _ command: String,
        queuedAt: ContinuousClock.Instant,
        startedAt: ContinuousClock.Instant
    ) {
        let finishedAt = ContinuousClock.now
        let phases = player.lastTimings()
        let message = String(
            format: "%@: espera %.0f ms, comando %.0f ms "
                + "(open %.1f, release %.1f, reopen %.1f, acquire %.1f, volume %.1f, "
                + "prefill %.1f, start %.1f)",
            command,
            milliseconds(startedAt - queuedAt),
            milliseconds(finishedAt - startedAt),
            phases.openMs, phases.releaseMs, phases.reopenMs, phases.acquireMs,
            phases.volumeMs, phases.prefillMs, phases.startMs
        )
        logger.notice("\(message, privacy: .public)")
    }

    private func advanceFromFinished(index: UInt32) {
        advancing = true
        advancedFinishedIndex = index
        let queuedAt = ContinuousClock.now
        let previous = serializedCommand
        transportCommandsInFlight += 1
        let task = Task {
            defer {
                advancing = false
                transportCommandsInFlight -= 1
            }
            await previous?.value
            guard !shuttingDown else { return }
            do {
                let advanced: TrackFormat? = try await timed("auto-avanço", queuedAt: queuedAt) {
                    [player] in try player.advance()
                }
                if let loaded = advanced {
                    apply(loaded: loaded)
                } else {
                    refresh()
                }
            } catch {
                errorMessage = "\(error)"
                refresh()
            }
        }
        serializedCommand = task
        pendingCommand = task
    }

    public func toggle() {
        guard !shuttingDown else { return }
        // O trabalho bloqueante (adquirir o hog, trocar o rate, esperar o hardware) roda
        // isolado num `Task.detached`, que só captura `player` — `Sendable` pelo protocolo
        // gerado. A task externa não é detached: herda a main actor, então é o lugar seguro
        // para tocar `self` de volta ao fim, sem violar o checking estrito de Swift 6.
        // Guardada em `pendingCommand` para que o comando seja observável sem depender de
        // relógio: quem precisar saber quando terminou aguarda a task, não um `sleep`.
        //
        // Encadeia com navegação e auto-avanço: todos podem soltar e readquirir o mesmo device,
        // então ordem de chegada precisa ser preservada fora da main actor.
        let queuedAt = ContinuousClock.now
        let previous = serializedCommand
        transportCommandsInFlight += 1
        let task = Task {
            defer { transportCommandsInFlight -= 1 }
            await previous?.value
            guard !shuttingDown else { return }
            let deveTocar = display.canPlay
            let devePausar = display.canPause
            guard deveTocar || devePausar else { return }
            do {
                try await timed(devePausar ? "pause" : "play", queuedAt: queuedAt) { [player] in
                    if devePausar {
                        try player.pause()
                    } else {
                        try player.play()
                    }
                }
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
        serializedCommand = task
        pendingCommand = task
    }

    public func applyVolume(_ scalar: Double) {
        guard !shuttingDown else { return }
        // Publica na hora: o slider não pode esperar o mutex do lado Rust — o mesmo que um
        // `play()` segura durante a aquisição inteira do hog — para responder ao arrasto.
        volume = scalar
        // O volume é a única coisa nesta interface que mexe em quanto sinal chega ao fone —
        // falhar calado aqui, como o shutdown, deixaria o usuário sem saber que o device não
        // mudou de verdade.
        //
        // O `Slider` chama isto continuamente durante o arrasto, não só ao soltar. Duas
        // exigências entram em conflito aqui: as escritas não podem chegar ao hardware fora
        // de ordem (o mutex do lado Rust dá exclusão mútua, não ordem de chegada — e ao
        // contrário de `play()`/`pause()`, `set_volume` não tem guarda de estado que reprove
        // um pedido atrasado), e o som tem de acompanhar o dedo.
        //
        // Encadear uma task por pedido resolvia a ordem e criava a fila: o custo virava
        // (pedidos × custo da escrita), drenando muito depois de o usuário soltar o controle.
        // Um worker só, com o pedido mais recente sobrescrevendo o anterior, resolve as duas:
        // a ordem é garantida por construção — só existe um escritor — e o arrasto todo
        // custa uma escrita, não uma por quadro. O hardware não precisa dos valores
        // intermediários, precisa do valor em que o dedo parou.
        volumeDesejado = scalar
        guard !escrevendoVolume else { return }
        escrevendoVolume = true
        pendingCommand = Task {
            defer { escrevendoVolume = false }
            while let alvo = volumeDesejado {
                volumeDesejado = nil
                do {
                    try await Task.detached { [player] in
                        try player.setVolume(scalar: Float(alvo))
                    }.value
                    errorMessage = nil
                } catch {
                    // O valor publicado tem de dizer a verdade: se o device recusou, o slider
                    // não pode continuar exibindo o pedido como se tivesse sido aceito. O
                    // snapshot é a fonte confiável do volume real, mantida pelo engine — nada
                    // de cache próprio. Pedidos ainda pendentes são descartados junto: insistir
                    // com o resto do arrasto por cima de uma recusa só repetiria o erro.
                    volumeDesejado = nil
                    volume = Double(player.snapshot().volumeScalar)
                    errorMessage = "\(error)"
                }
            }
        }
    }

    public func shutdown() {
        guard !shuttingDown else { return }
        shuttingDown = true
        stopPolling()
        entryLoading?.cancel()
        metadataLoading?.cancel()
        pendingEntryPaths.removeAll()
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

/// Os passos somados de uma navegação que ainda não chegou ao Rust. Classe porque os cliques
/// seguintes alteram a mesma instância que a task já capturou; isolada na main actor, como
/// tudo o que a toca.
@MainActor
private final class NavigationRequest {
    var steps: Int32

    init(steps: Int32) {
        self.steps = steps
    }
}
