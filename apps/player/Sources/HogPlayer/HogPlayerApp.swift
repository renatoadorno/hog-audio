import AppKit
import Darwin
import Dispatch
import HogPlayerKit
import SwiftUI

@main
struct HogPlayerApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        Window("hog-audio", id: "player") {
            ContentView(model: delegate.model)
                .onAppear {
                    delegate.model.startPolling()
                    // Argumento de linha de comando: é o que permite iterar rápido durante o
                    // desenvolvimento sem passar pelo painel de abrir arquivo.
                    if let path = CommandLine.arguments.dropFirst().first {
                        delegate.model.open(url: URL(fileURLWithPath: path))
                    }
                }
        }
        // O conteúdo declara só o mínimo: acima dele a janela cresce e a capa acompanha.
        .windowResizability(.contentMinSize)
        .defaultSize(width: 920, height: 700)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    // Criado junto com o delegate, antes de qualquer cena existir: não há instante em que o
    // encerramento do processo encontre o modelo nulo.
    let model = PlayerViewModel()

    // `DispatchSourceSignal` é cancelado se for desalocado — sem retê-las aqui, os handlers
    // instalados em `installSignalHandlers()` nunca disparariam.
    private var signalSources: [DispatchSourceSignal] = []

    func applicationDidFinishLaunching(_ notification: Notification) {
        installSignalHandlers()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func applicationWillTerminate(_ notification: Notification) {
        // Sem isto o device fica travado no rate e no formato da última faixa.
        model.shutdown()
    }

    // `make run-app` — o loop de desenvolvimento documentado no README — roda o binário em
    // primeiro plano, preso ao terminal: Ctrl+C manda SIGINT direto ao processo, sem passar
    // por `applicationWillTerminate`. O mesmo vale para SIGTERM, para SIGHUP (fechar a janela
    // do terminal) e para SIGQUIT (Ctrl+\). A CLI deste projeto já protege os mesmos quatro
    // sinais; a interface não podia chegar menos protegida que o binário que ela substitui.
    private func installSignalHandlers() {
        for sig in [SIGINT, SIGTERM, SIGHUP, SIGQUIT] {
            // Descarta a disposição default do sinal: sem isto o processo morre antes de o
            // `DispatchSource` abaixo ter qualquer chance de rodar.
            signal(sig, SIG_IGN)
            let source = DispatchSource.makeSignalSource(signal: sig, queue: .main)
            source.setEventHandler { [weak self] in
                // Um handler POSIX direto seria async-signal-unsafe; o `DispatchSource`
                // entrega o evento em contexto normal — mas o compilador só reconhece isolamento
                // de main actor num salto explícito, daí o `Task { @MainActor in ... }`.
                Task { @MainActor in
                    self?.model.shutdown()
                    NSApp.terminate(nil)
                }
            }
            source.resume()
            signalSources.append(source)
        }
    }
}
