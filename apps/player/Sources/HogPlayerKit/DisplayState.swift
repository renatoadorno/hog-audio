import Foundation
import HogAudioBindings

/// O que a tela mostra — distinto de `Snapshot`, que é o estado do engine. `Snapshot` fala a
/// língua do motor de áudio; `DisplayState` já vem traduzido para o que a interface renderiza.
public struct DisplayState: Equatable, Sendable {
    public let title: String
    public let artist: String
    public let album: String
    public let artwork: Data?
    public let elapsed: String
    public let total: String
    public let progress: Double
    public let canPlay: Bool
    public let canPause: Bool
    public let canGoNext: Bool
    public let canGoPrevious: Bool
    public let isPlaying: Bool
    public let technicalLine: String
}

/// Traduz o estado do engine para o que a tela mostra. É função pura de propósito: a interface
/// consulta dez vezes por segundo, e nenhuma dessas consultas pode depender de hardware para
/// ser testada.
public func displayState(
    snapshot: Snapshot,
    metadata: TrackMetadata?,
    format: TrackFormat?
) -> DisplayState {
    let tocando = snapshot.state == .playing
    // Em Loaded a negociação já rodou, mas o hog só é tomado no play. Em Finished o
    // `poll_finished` do lado Rust só chama `AudioDeviceStop` — o device continua retido, e
    // é exatamente o estado em que a surpresa de "por que o Mac segue mudo" é maior.
    let comDevice = [.playing, .paused, .finished].contains(snapshot.state)

    var tecnica = ""
    if let format {
        var partes = [
            qualityDetail(
                sampleRate: format.sampleRate,
                bitDepth: format.bitDepth,
                codec: format.codec
            ),
        ]
        if comDevice { partes.append("hog ativo") }
        if snapshot.underruns > 0 { partes.append("\(snapshot.underruns) underruns") }
        tecnica = partes.joined(separator: " · ")
    }

    let progresso: Double
    if snapshot.totalSeconds > 0 {
        progresso = min(1.0, max(0.0, snapshot.elapsedSeconds / snapshot.totalSeconds))
    } else {
        progresso = 0
    }

    return DisplayState(
        title: metadata?.title ?? "nenhuma faixa carregada",
        artist: metadata?.artist ?? "",
        album: metadata?.album ?? "",
        artwork: metadata?.artwork,
        elapsed: formatTime(snapshot.elapsedSeconds),
        total: formatTime(snapshot.totalSeconds),
        progress: progresso,
        canPlay: [.loaded, .paused, .finished].contains(snapshot.state),
        canPause: tocando,
        canGoNext: snapshot.currentIndex.map { $0 + 1 < snapshot.queueLen } ?? false,
        canGoPrevious: snapshot.currentIndex.map {
            $0 > 0 || snapshot.elapsedSeconds >= 3.0
        } ?? false,
        isPlaying: tocando,
        technicalLine: tecnica
    )
}
