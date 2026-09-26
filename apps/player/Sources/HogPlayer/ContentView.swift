import AppKit
import CoreImage
import HogPlayerKit
import SwiftUI
import UniformTypeIdentifiers

private final class DropCollector: @unchecked Sendable {
    private let lock = NSLock()
    private var remaining: Int
    private var urls: [URL] = []
    private var failures: [String] = []

    init(count: Int) { remaining = count }

    func finish(url: URL?, failure: String?) -> (urls: [URL], failure: String?)? {
        lock.lock(); defer { lock.unlock() }
        if let url { urls.append(url) }
        if let failure { failures.append(failure) }
        remaining -= 1
        guard remaining == 0 else { return nil }
        return (urls, urls.isEmpty ? failures.first : nil)
    }
}

extension Color {
    static var hogAccent: Color { Color(red: 0.98, green: 0.38, blue: 0.07) }
}

/// A capa decodificada e o fundo já borrado, gerados uma vez por faixa e fora da main thread:
/// decodificar uma capa grande e borrar levava dezenas de milissegundos na troca de faixa.
private struct Artwork: Sendable {
    let cover: CGImage
    let backdrop: CGImage?

    /// A coluna tem no máximo 560 pt (1120 px em Retina); uma capa embutida de 3000 px
    /// decodificada inteira ocuparia 36 MB para ser desenhada a um terço do tamanho.
    init?(data: Data) {
        guard let cover = makeThumbnail(from: data, maxPixelSize: 1200) else { return nil }
        self.cover = cover
        self.backdrop = Self.blurred(data)
    }

    /// O fundo não precisa de detalhe: reduzir antes de borrar deixa o blur barato, e a
    /// ampliação na tela só suaviza mais.
    private static func blurred(_ data: Data) -> CGImage? {
        guard let small = makeThumbnail(from: data, maxPixelSize: 160) else { return nil }
        let input = CIImage(cgImage: small)
        let blurred = input.clampedToExtent()
            .applyingGaussianBlur(sigma: 10)
            .cropped(to: input.extent)
        return CIContext().createCGImage(blurred, from: input.extent)
    }
}

struct ContentView: View {
    @ObservedObject var model: PlayerViewModel
    @State private var dropError: String?
    @State private var selectedIndex: UInt32?
    @State private var isDropTarget = false
    @State private var artwork: Artwork?

    private let accent = Color.hogAccent

    var body: some View {
        ZStack {
            ArtworkBackdrop(image: artwork?.backdrop)

            GeometryReader { proxy in
                let coverSide = PlayerLayout.coverSide(in: proxy.size)
                HStack(alignment: .top, spacing: PlayerLayout.columnSpacing) {
                    nowPlayingColumn(coverSide: coverSide)
                        .frame(width: coverSide)
                        .frame(maxHeight: .infinity, alignment: .top)
                    VStack(spacing: 12) {
                        statusArea
                        queuePanel
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                }
                .padding(PlayerLayout.padding)
            }
        }
        .frame(minWidth: PlayerLayout.minWindow.width, minHeight: PlayerLayout.minWindow.height)
        .tint(accent)
        .toolbarBackground(.hidden, for: .windowToolbar)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button(action: chooseFile) {
                    Label("Abrir", systemImage: "plus")
                        .labelStyle(.titleAndIcon)
                        .padding(.horizontal, 8)
                }
                .keyboardShortcut("o")
                .help("Abrir arquivos ou pastas")
            }
        }
        .overlay {
            RoundedRectangle(cornerRadius: 12)
                .stroke(accent.opacity(isDropTarget ? 0.9 : 0), lineWidth: 3)
                .padding(8)
        }
        .onDrop(of: [.fileURL], isTargeted: $isDropTarget, perform: handleDrop)
        .onChange(of: model.currentIndex) { _, index in selectedIndex = index }
        .task(id: model.display.artwork) {
            let data = model.display.artwork
            let decoded = await Task.detached { data.flatMap(Artwork.init(data:)) }.value
            // Uma troca rápida de faixa cancela esta task; a capa que chegou atrasada não vale.
            guard !Task.isCancelled else { return }
            artwork = decoded
        }
        .animation(.easeOut(duration: 0.18), value: isDropTarget)
    }

    private func nowPlayingColumn(coverSide: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            // `fit` é a rede de segurança do `controlsHeight` estimado: se os controles
            // ocuparem mais do que o previsto, a capa encolhe em vez de empurrá-los para fora.
            CoverArt(image: artwork?.cover)
                .aspectRatio(1, contentMode: .fit)
                .frame(maxWidth: coverSide, maxHeight: coverSide)
            trackInfo
            clock
            transport
            volume
            if !model.display.technicalLine.isEmpty {
                Text(model.display.technicalLine)
                    .font(.caption2.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
    }

    private var trackInfo: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(model.display.title)
                .font(.system(.title2, design: .rounded, weight: .bold))
                .lineLimit(1)
                .help(model.display.title)
            if !model.display.artist.isEmpty {
                Text(model.display.artist)
                    .font(.headline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            if !model.display.album.isEmpty {
                Text(model.display.album)
                    .font(.subheadline)
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
            }
        }
    }

    private var clock: some View {
        VStack(spacing: 4) {
            ProgressView(value: model.display.progress)
                .progressViewStyle(.linear)
            HStack {
                Text(model.display.elapsed)
                Spacer()
                Text(model.display.total)
            }
            .font(.caption.monospacedDigit())
            .foregroundStyle(.secondary)
        }
    }

    private var transport: some View {
        HStack(spacing: 24) {
            transportButton(
                systemName: "backward.fill",
                label: "Faixa anterior",
                enabled: model.display.canGoPrevious,
                action: model.previous
            )
            Button(action: model.toggle) {
                Image(systemName: model.display.isPlaying ? "pause.fill" : "play.fill")
                    .font(.system(size: 24, weight: .bold))
                    .foregroundStyle(.white)
                    .frame(width: 56, height: 56)
                    .background(accent, in: Circle())
                    .shadow(color: accent.opacity(0.35), radius: 12, y: 4)
            }
            .buttonStyle(.plain)
            .disabled(!model.display.canPlay && !model.display.canPause)
            .opacity((model.display.canPlay || model.display.canPause) ? 1 : 0.4)
            .keyboardShortcut(.space, modifiers: [])
            .accessibilityLabel(model.display.isPlaying ? "Pausar" : "Tocar")
            transportButton(
                systemName: "forward.fill",
                label: "Próxima faixa",
                enabled: model.display.canGoNext,
                action: model.next
            )
        }
        .frame(maxWidth: .infinity)
    }

    private func transportButton(
        systemName: String,
        label: String,
        enabled: Bool,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Image(systemName: systemName)
                .font(.system(size: 16, weight: .semibold))
                .frame(width: 40, height: 40)
                .background(Color.primary.opacity(0.08), in: Circle())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.32)
        .accessibilityLabel(label)
    }

    private var volume: some View {
        HStack(spacing: 8) {
            Image(systemName: model.volume < 0.01 ? "speaker.slash.fill" : "speaker.wave.2.fill")
                .foregroundStyle(accent)
                .frame(width: 20)
            Slider(value: Binding(
                get: { model.volume },
                set: { model.applyVolume($0) }
            ), in: 0...1)
            Text("\(Int(model.volume * 100))%")
                .font(.caption.monospacedDigit().weight(.semibold))
                .foregroundStyle(.secondary)
                .frame(width: 40, alignment: .trailing)
        }
    }

    private var queuePanel: some View {
        VStack(spacing: 8) {
            HStack {
                Text("FILA")
                    .font(.caption.weight(.bold))
                    .foregroundStyle(.secondary)
                Text("\(model.queue.count)")
                    .font(.caption2.monospacedDigit().weight(.bold))
                    .foregroundStyle(accent)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(accent.opacity(0.12), in: Capsule())
                Spacer()
                Button("Limpar", action: model.clearQueue)
                    .buttonStyle(.plain)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(model.queue.isEmpty ? Color.secondary : accent)
                    .disabled(model.queue.isEmpty)
            }
            .padding(.horizontal, 4)
            if model.queue.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "music.note.list")
                        .font(.title2)
                        .foregroundStyle(accent.opacity(0.8))
                    Text("Arraste músicas ou uma pasta para começar")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                queue
            }
        }
        .padding(12)
        .frame(maxHeight: .infinity)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
    }

    private var queue: some View {
        ScrollViewReader { proxy in
            List(selection: $selectedIndex) {
                ForEach(Array(model.queue.enumerated()), id: \.offset) { index, item in
                    queueRow(index: index, path: item.path, failure: item.failure)
                        .tag(UInt32(index))
                        .contentShape(Rectangle())
                        .onTapGesture {
                            selectedIndex = UInt32(index)
                            model.selectTrack(index: UInt32(index))
                        }
                }
            }
            .listStyle(.inset)
            .scrollContentBackground(.hidden)
            .frame(maxHeight: .infinity)
            .onDeleteCommand {
                guard let selectedIndex else { return }
                self.selectedIndex = nil
                model.removeTrack(index: selectedIndex)
            }
            // A lista chega inteira de uma vez e abre rolada até o fim, escondendo o início;
            // no auto-avanço, a faixa atual sairia de vista. Levar a rolagem até ela resolve
            // os dois.
            .onChange(of: model.currentIndex, initial: true) { _, index in
                guard let index else { return }
                proxy.scrollTo(Int(index))
            }
        }
    }

    private func queueRow(index: Int, path: String, failure: String?) -> QueueRow {
        let isCurrent = UInt32(index) == model.currentIndex
        return QueueRow(
            entry: model.entries[path],
            fallbackName: URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent,
            isCurrent: isCurrent,
            // Só a linha atual depende do play/pause: passar o estado a todas faria a fila
            // inteira redesenhar a cada toque no botão.
            isPlaying: isCurrent && model.display.isPlaying,
            failure: failure
        )
    }

    @ViewBuilder
    private var statusArea: some View {
        if let message = model.errorMessage ?? dropError {
            HStack(spacing: 8) {
                Image(systemName: "exclamationmark.circle.fill")
                Text(message).lineLimit(2)
                Spacer()
            }
            .font(.caption)
            .foregroundStyle(.red)
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(.red.opacity(0.09), in: RoundedRectangle(cornerRadius: 8))
        }
    }

    private func chooseFile() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = true
        panel.allowedContentTypes = [.audio]
        if panel.runModal() == .OK {
            // Sem isto, um erro de drop anterior fica preso na tela mesmo depois de um
            // arquivo válido carregar com sucesso pelo painel.
            dropError = nil
            model.add(urls: panel.urls)
        }
    }

    private func handleDrop(_ providers: [NSItemProvider]) -> Bool {
        guard !providers.isEmpty else { return false }
        let collector = DropCollector(count: providers.count)
        for provider in providers {
            _ = provider.loadObject(ofClass: URL.self) { url, error in
                let failureReason = error.map(String.init(describing:))
                guard let result = collector.finish(url: url, failure: failureReason) else {
                    return
                }
                Task { @MainActor in
                    guard !result.urls.isEmpty else {
                        dropError = result.failure ?? "não foi possível ler os itens arrastados"
                        return
                    }
                    dropError = nil
                    model.add(urls: result.urls)
                }
            }
        }
        return true
    }
}

/// Struct própria, e não propriedade computada do `ContentView`: com a mesma imagem, o
/// SwiftUI pula este body nos ticks do poll.
private struct CoverArt: View {
    let image: CGImage?

    var body: some View {
        // `Color.clear` fixa o tamanho proposto; a imagem em `fill` transborda por cima e o
        // recorte devolve o quadrado — sem isso o `fill` inflaria o layout da coluna.
        Color.clear
            .overlay {
                if let image {
                    Image(decorative: image, scale: 2)
                        .resizable()
                        .scaledToFill()
                } else {
                    LinearGradient(
                        colors: [.hogAccent, Color(red: 0.72, green: 0.18, blue: 0.02)],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                    .overlay {
                        Image(systemName: "waveform")
                            .font(.system(size: 64, weight: .semibold))
                            .foregroundStyle(.white.opacity(0.92))
                    }
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay {
                RoundedRectangle(cornerRadius: 12)
                    .stroke(.white.opacity(0.12), lineWidth: 1)
            }
            .shadow(color: .black.opacity(0.3), radius: 24, y: 12)
    }
}

/// Fundo da janela: a capa da faixa atual, já borrada, sob um véu da cor da janela que mantém
/// o texto legível no modo claro e no escuro. Sem capa, o gradiente discreto de antes.
private struct ArtworkBackdrop: View {
    let image: CGImage?

    var body: some View {
        LinearGradient(
            colors: [Color(nsColor: .windowBackgroundColor), Color.hogAccent.opacity(0.055)],
            startPoint: .top,
            endPoint: .bottomTrailing
        )
        .overlay {
            if let image {
                Image(decorative: image, scale: 1)
                    .resizable()
                    .interpolation(.high)
                    .scaledToFill()
                    .overlay(Color(nsColor: .windowBackgroundColor).opacity(0.6))
                    .id(ObjectIdentifier(image))
                    .transition(.opacity)
            }
        }
        .clipped()
        .animation(.easeInOut(duration: 0.4), value: image.map(ObjectIdentifier.init))
        .ignoresSafeArea()
    }
}
