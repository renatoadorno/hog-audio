import Foundation
import HogAudioBindings
import Observation
import os

// O protocolo que o ViewModel consome é o `HogPlayerProtocol` **gerado pelo uniffi** — ele já
// declara exatamente a fronteira atual e já é `Sendable`. Escrever um protocolo próprio aqui
// duplicaria a fronteira e sairia de sincronia no dia em que a API do Rust mudasse.

private let logger = Logger(subsystem: "local.hogaudio.player", category: "PlayerViewModel")

private func milliseconds(_ duration: Duration) -> Double {
    let (seconds, attoseconds) = duration.components
    return Double(seconds) * 1000 + Double(attoseconds) / 1e15
}

/// Só o que a tela lê é observado; o resto é `@ObservationIgnored`. Uma atribuição de valor
/// igual não notifica: a macro compara os tipos `Equatable` antes de avisar quem observa.
@Observable
@MainActor
public final class PlayerViewModel {
    public private(set) var display: DisplayState
    /// Separado de `display` porque muda a cada tique enquanto toca.
    public private(set) var clock: ClockState
    public var volume: Double = 0.5
    public private(set) var muted = false
    public private(set) var errorMessage: String?
    public private(set) var queue: [QueueItem] = []
    public private(set) var currentIndex: UInt32?
    /// Título, capa, qualidade e duração de cada faixa da fila, pelo caminho. Chega aos poucos
    /// depois que a faixa entra na fila; até lá a linha mostra o nome do arquivo.
    public private(set) var entries: [String: QueueEntryInfo] = [:]

    /// A carga das linhas em andamento, exposta pelo mesmo motivo de `pendingCommand`: quem
    /// precisar saber quando terminou aguarda a task em vez de adivinhar por tempo.
    @ObservationIgnored public private(set) var entryLoading: Task<Void, Never>?

    /// A task do último comando emitido. Existe para que quem precise saber quando o comando
    /// terminou possa aguardá-la, em vez de adivinhar por tempo — e para que dois comandos
    /// consecutivos sejam observáveis em ordem.
    ///
    /// Os comandos de transporte são encadeados por `serializedCommand`; o worker de volume
    /// pode rodar em paralelo porque coalesce pedidos e o mutex do Rust protege o hardware.
    /// `pendingCommand` continua expondo a task mais recente para testes e desligamento.
    @ObservationIgnored public private(set) var pendingCommand: Task<Void, Never>?
    @ObservationIgnored private var serializedCommand: Task<Void, Never>?

    private let player: any HogPlayerProtocol
    private let loadEntry: @Sendable (String) async -> QueueEntryInfo
    @ObservationIgnored private var metadata: TrackMetadata?
    @ObservationIgnored private var format: TrackFormat?
    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private var lastQueueVersion: UInt64?
    @ObservationIgnored private var advancing = false
    @ObservationIgnored private var advancedFinishedIndex: UInt32?
    @ObservationIgnored private var transportCommandsInFlight = 0
    @ObservationIgnored private var shuttingDown = false
    /// O último problema do engine já copiado para `errorMessage`, e se uma recuperação do
    /// device está na fila de comandos.
    @ObservationIgnored private var lastProblemSeq: UInt64 = 0
    @ObservationIgnored private var recovering = false
    /// O `lastProblemSeq` quando o comando de transporte atual começou. Os comandos de
    /// transporte são encadeados, então um valor só basta.
    @ObservationIgnored private var problemSeqAtCommandStart: UInt64 = 0

    /// O volume que o usuário pediu por último e que ainda não foi ao hardware, e se já há um
    /// worker drenando. Ambos vivem na main actor, então o arrasto os atualiza sem trava.
    /// Ver `applyVolume()`.
    @ObservationIgnored private var volumeDesejado: Double?
    @ObservationIgnored private var escrevendoVolume = false
    /// O usuário está com o slider na mão.
    @ObservationIgnored private var volumeEditing = false

    /// Soltar uma pasta de mil faixas não pode disparar mil leituras de tag e decodificações
    /// de capa ao mesmo tempo: quatro mantêm a lista enchendo de cima para baixo sem disputar
    /// CPU e disco com a reprodução.
    private let maxConcurrentEntryLoads = 4
    @ObservationIgnored private var pendingEntryPaths: [String] = []
    @ObservationIgnored private var loadingEntryPaths: Set<String> = []
    @ObservationIgnored private var queuedPaths: Set<String> = []

    /// Os metadados completos (com a capa em resolução cheia) da faixa atual carregam fora da
    /// cadeia de comandos: o próximo Next não espera por eles. `metadataPath` diz de qual
    /// faixa são os que estão em `metadata`.
    private let metadataLoader: @Sendable (URL) async -> TrackMetadata
    @ObservationIgnored private var metadataPath: String?
    @ObservationIgnored public private(set) var metadataLoading: Task<Void, Never>?

    /// A navegação que ainda espera o comando anterior terminar. Cliques novos somam nela em
    /// vez de enfileirar outra carga; `nil` quando nenhuma está esperando.
    @ObservationIgnored private var pendingNavigation: NavigationRequest?

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
        self.clock = clockState(snapshot: snapshot)
        self.currentIndex = snapshot.currentIndex
        self.queue = player.queueItems()
        self.lastQueueVersion = snapshot.queueVersion
        // Um problema de antes de a janela abrir não é notícia.
        self.lastProblemSeq = snapshot.problemSeq
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
        // Folga para o sistema agrupar este disparo com outros e acordar a CPU menos vezes;
        // 20 ms num relógio de 100 ms não se vê.
        timer.tolerance = 0.02
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
        currentIndex = snapshot.currentIndex
        // `tryQueueItems` nunca espera o engine: com um comando segurando o lock (troca de
        // device leva segundos), a versão não avança e o próximo tique tenta de novo.
        if lastQueueVersion != snapshot.queueVersion, let items = player.tryQueueItems() {
            queue = items
            lastQueueVersion = snapshot.queueVersion
            syncEntries()
        }
        display = displayState(
            snapshot: snapshot,
            metadata: shownMetadata(for: snapshot.currentIndex),
            format: format
        )
        clock = clockState(snapshot: snapshot)
        muted = snapshot.muted
        followDeviceVolume(snapshot)

        if snapshot.problemSeq != lastProblemSeq {
            lastProblemSeq = snapshot.problemSeq
            if let problem = player.lastProblem() {
                errorMessage = problem
            }
        }
        // Antes do auto-avanço: a recuperação entra em `transportCommandsInFlight`, e é isso
        // que o impede de tocar a faixa seguinte no device que o macOS pôs no lugar do perdido.
        if snapshot.deviceSuspect, !recovering, !shuttingDown {
            recoverDevice()
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

    /// Um comando bem-sucedido limpa o erro de antes dele, mas não um problema que o engine
    /// publicou durante o próprio comando e um tique do poll já mostrou — o aviso de volume
    /// que voltou ao teto sai no fim do Play.
    private func clearErrorFromBeforeCommand() {
        if lastProblemSeq == problemSeqAtCommandStart {
            errorMessage = nil
        }
    }

    /// O engine só detecta a perda no poll; confirmar e parar é um comando, na mesma fila
    /// dos outros, fora da main actor.
    private func recoverDevice() {
        recovering = true
        let previous = serializedCommand
        transportCommandsInFlight += 1
        let task = Task {
            defer {
                recovering = false
                transportCommandsInFlight -= 1
            }
            await previous?.value
            guard !shuttingDown else { return }
            // A única falha possível é o player já estar encerrando.
            _ = try? await Task.detached { [player] in try player.recover() }.value
            refresh()
        }
        serializedCommand = task
        pendingCommand = task
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

    /// A linha da faixa atual: é por ela que a fila marca a seleção e rola até a atual.
    public var currentItemID: UInt64? {
        guard let currentIndex, Int(currentIndex) < queue.count else { return nil }
        return queue[Int(currentIndex)].id
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
                // falha da carga inicial. A seleção explícita torna esse erro visível. A fila
                // estava vazia e os comandos são encadeados: a posição 0 é a faixa que entrou.
                await loadSelection(command: "carga inicial", queuedAt: queuedAt) { [player] in
                    try player.selectTrack(index: 0)
                }
            }
        }
        serializedCommand = task
        pendingCommand = task
    }

    /// Por id, não por índice: a fila da tela pode estar defasada em relação à do engine, e
    /// o índice de uma linha muda a cada remoção.
    public func selectTrack(id: UInt64) {
        guard !shuttingDown else { return }
        let queuedAt = ContinuousClock.now
        let previous = serializedCommand
        transportCommandsInFlight += 1
        let task = Task {
            defer { transportCommandsInFlight -= 1 }
            await previous?.value
            guard !shuttingDown else { return }
            await loadSelection(command: "seleção", queuedAt: queuedAt) { [player] in
                try player.selectTrackId(id: id)
            }
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

    public func removeTrack(id: UInt64) {
        guard !shuttingDown else { return }
        let queuedAt = ContinuousClock.now
        let previous = serializedCommand
        transportCommandsInFlight += 1
        let task = Task {
            defer { transportCommandsInFlight -= 1 }
            await previous?.value
            guard !shuttingDown else { return }
            let removedCurrent = currentItemID == id
            do {
                // Removida a atual, o engine já carregou quem assumiu a posição e devolve o
                // formato dela.
                let loaded = try await timed("remoção", queuedAt: queuedAt) { [player] in
                    try player.removeTrackId(id: id)
                }
                if let loaded {
                    apply(loaded: loaded)
                } else {
                    clearErrorFromBeforeCommand()
                    if removedCurrent {
                        metadataLoading?.cancel()
                        metadata = nil
                        metadataPath = nil
                        format = nil
                    }
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
        command: String,
        queuedAt: ContinuousClock.Instant,
        _ select: @escaping @Sendable () throws -> TrackFormat
    ) async {
        do {
            let loaded = try await timed(command, queuedAt: queuedAt, select)
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
        clearErrorFromBeforeCommand()
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
        problemSeqAtCommandStart = lastProblemSeq
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
                clearErrorFromBeforeCommand()
            } catch {
                errorMessage = "\(error)"
            }
            // Só o `play()` publica volume no `SharedStatus` do lado Rust — quando o teto de
            // segurança agiu, é aqui, ao fim do comando, que o slider precisa reler o valor
            // real do hardware.
            if let scalar = player.snapshot().volumeScalar {
                volume = Double(scalar)
            }
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
                    if let scalar = player.snapshot().volumeScalar {
                        volume = Double(scalar)
                    }
                    errorMessage = "\(error)"
                }
            }
        }
    }

    public func setVolumeEditing(_ editing: Bool) {
        volumeEditing = editing
    }

    /// O slider acompanha o volume mudado por fora só quando nada nosso mexe nele: fora de
    /// Playing e Paused o device está sendo trocado ou já não é nosso, e durante um arrasto,
    /// uma escrita ou um comando o hardware ainda tem o valor de antes.
    private func followDeviceVolume(_ snapshot: Snapshot) {
        guard let scalar = snapshot.volumeScalar,
              snapshot.state == .playing || snapshot.state == .paused,
              !volumeEditing,
              !escrevendoVolume,
              volumeDesejado == nil,
              transportCommandsInFlight == 0
        else { return }
        // A mesma tolerância da confirmação de escrita do engine: abaixo dela é o volume que
        // nós mesmos pedimos, só que ida e volta por `Float`.
        let device = Double(scalar)
        if abs(device - volume) > 0.005 {
            volume = device
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
