import AppKit
import HogPlayerKit
import SwiftUI

@main
struct HogPlayerApp: App {
    @StateObject private var model = PlayerViewModel()
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        Window("hog-audio", id: "player") {
            ContentView(model: model)
                .onAppear {
                    delegate.model = model
                    model.startPolling()
                    // Argumento de linha de comando: é o que permite iterar rápido durante o
                    // desenvolvimento sem passar pelo painel de abrir arquivo.
                    if let caminho = CommandLine.arguments.dropFirst().first {
                        model.open(url: URL(fileURLWithPath: caminho))
                    }
                }
        }
        .windowResizability(.contentSize)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    @MainActor var model: PlayerViewModel?

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func applicationWillTerminate(_ notification: Notification) {
        // Sem isto o device fica travado no rate e no formato da última faixa.
        MainActor.assumeIsolated { model?.shutdown() }
    }
}
