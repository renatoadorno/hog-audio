import AppKit
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
        .windowResizability(.contentSize)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    // Criado junto com o delegate, antes de qualquer cena existir: não há instante em que o
    // encerramento do processo encontre o modelo nulo.
    let model = PlayerViewModel()

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func applicationWillTerminate(_ notification: Notification) {
        // Sem isto o device fica travado no rate e no formato da última faixa.
        model.shutdown()
    }
}
