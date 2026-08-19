import AppKit
import HogPlayerKit
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @ObservedObject var model: PlayerViewModel
    @State private var dropError: String?

    var body: some View {
        VStack(spacing: 14) {
            coverArt
            trackInfo
            clock
            controls
            volume
            footer
        }
        .padding(18)
        .frame(width: 360, height: 480)
        .onDrop(of: [.fileURL], isTargeted: nil, perform: handleDrop)
    }

    private var coverArt: some View {
        Group {
            if let data = model.display.artwork, let image = NSImage(data: data) {
                Image(nsImage: image).resizable().aspectRatio(contentMode: .fit)
            } else {
                RoundedRectangle(cornerRadius: 6)
                    .fill(Color.secondary.opacity(0.15))
                    .overlay(Image(systemName: "music.note").font(.largeTitle)
                        .foregroundStyle(.secondary))
            }
        }
        .frame(width: 220, height: 220)
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    private var trackInfo: some View {
        VStack(spacing: 3) {
            Text(model.display.title).font(.headline).lineLimit(1)
            Text(model.display.artist).font(.subheadline)
                .foregroundStyle(.secondary).lineLimit(1)
            Text(model.display.album).font(.caption)
                .foregroundStyle(.tertiary).lineLimit(1)
        }
    }

    private var clock: some View {
        VStack(spacing: 4) {
            ProgressView(value: model.display.progress)
            HStack {
                Text(model.display.elapsed)
                Spacer()
                Text(model.display.total)
            }
            .font(.caption.monospacedDigit())
            .foregroundStyle(.secondary)
        }
    }

    private var controls: some View {
        HStack(spacing: 20) {
            Button("Abrir…", action: chooseFile)
            Button(action: model.toggle) {
                Image(systemName: model.display.isPlaying ? "pause.fill" : "play.fill")
                    .font(.title)
                    .frame(width: 44, height: 44)
            }
            .disabled(!model.display.canPlay && !model.display.canPause)
            .keyboardShortcut(.space, modifiers: [])
        }
    }

    private var volume: some View {
        HStack {
            Image(systemName: "speaker.fill").foregroundStyle(.secondary)
            Slider(value: Binding(
                get: { model.volume },
                set: { model.applyVolume($0) }
            ), in: 0...1)
            Text("\(Int(model.volume * 100))%")
                .font(.caption.monospacedDigit())
                .frame(width: 40, alignment: .trailing)
        }
    }

    private var footer: some View {
        VStack(spacing: 4) {
            Text(model.display.technicalLine)
                .font(.caption2.monospaced())
                .foregroundStyle(.secondary)
            // `dropError` cobre a falha do NSItemProvider, que nunca chega a `model.open` —
            // `model.errorMessage` tem setter privado em HogPlayerKit e não pode ser escrito
            // daqui, então a falha do drop precisa do próprio estado local para não morrer muda.
            if let message = model.errorMessage ?? dropError {
                Text(message).font(.caption2).foregroundStyle(.red)
                    .lineLimit(3).multilineTextAlignment(.center)
            }
        }
        .frame(height: 48)
    }

    private func chooseFile() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.allowedContentTypes = [.audio]
        if panel.runModal() == .OK, let url = panel.url {
            // Sem isto, um erro de drop anterior fica preso na tela mesmo depois de um
            // arquivo válido carregar com sucesso pelo painel.
            dropError = nil
            model.open(url: url)
        }
    }

    private func handleDrop(_ providers: [NSItemProvider]) -> Bool {
        guard let provider = providers.first else { return false }
        _ = provider.loadObject(ofClass: URL.self) { url, error in
            // Convertido para `String` ainda fora da task: `Error` não é `Sendable` e não pode
            // atravessar para uma closure isolada à main actor sob concorrência estrita.
            let failureReason = error.map(String.init(describing:))
            Task { @MainActor in
                guard let url else {
                    dropError = failureReason ?? "não foi possível ler o arquivo arrastado"
                    return
                }
                dropError = nil
                model.open(url: url)
            }
        }
        return true
    }
}
