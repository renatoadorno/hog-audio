import AppKit
import HogPlayerKit
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @ObservedObject var model: PlayerViewModel

    var body: some View {
        VStack(spacing: 14) {
            capa
            informacoes
            relogio
            controles
            volume
            rodape
        }
        .padding(18)
        .frame(width: 360, height: 480)
        .onDrop(of: [.fileURL], isTargeted: nil, perform: receberArquivo)
    }

    private var capa: some View {
        Group {
            if let dados = model.display.artwork, let imagem = NSImage(data: dados) {
                Image(nsImage: imagem).resizable().aspectRatio(contentMode: .fit)
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

    private var informacoes: some View {
        VStack(spacing: 3) {
            Text(model.display.title).font(.headline).lineLimit(1)
            Text(model.display.artist).font(.subheadline)
                .foregroundStyle(.secondary).lineLimit(1)
            Text(model.display.album).font(.caption)
                .foregroundStyle(.tertiary).lineLimit(1)
        }
    }

    private var relogio: some View {
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

    private var controles: some View {
        HStack(spacing: 20) {
            Button("Abrir…", action: escolherArquivo)
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

    private var rodape: some View {
        VStack(spacing: 4) {
            Text(model.display.technicalLine)
                .font(.caption2.monospaced())
                .foregroundStyle(.secondary)
            if let erro = model.errorMessage {
                Text(erro).font(.caption2).foregroundStyle(.red)
                    .lineLimit(3).multilineTextAlignment(.center)
            }
        }
        .frame(height: 48)
    }

    private func escolherArquivo() {
        let painel = NSOpenPanel()
        painel.allowsMultipleSelection = false
        painel.canChooseDirectories = false
        painel.allowedContentTypes = [.audio]
        if painel.runModal() == .OK, let url = painel.url {
            model.open(url: url)
        }
    }

    private func receberArquivo(_ provedores: [NSItemProvider]) -> Bool {
        guard let provedor = provedores.first else { return false }
        _ = provedor.loadObject(ofClass: URL.self) { url, _ in
            guard let url else { return }
            Task { @MainActor in model.open(url: url) }
        }
        return true
    }
}
