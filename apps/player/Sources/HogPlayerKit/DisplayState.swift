import Foundation
import HogAudioBindings

public struct DisplayState: Equatable {
    public let title: String
    public let artist: String
    public let album: String
    public let artwork: Data?
    public let elapsed: String
    public let total: String
    public let progress: Double
    public let canPlay: Bool
    public let canPause: Bool
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
    // Só estes dois estados seguram o device: em Loaded a negociação já rodou, mas o hog só
    // é tomado no play.
    let comDevice = snapshot.state == .playing || snapshot.state == .paused

    var tecnica = ""
    if let format {
        var partes = [
            String(format: "%.0f kHz", format.sampleRate / 1000),
            "\(format.bitDepth) bit",
            format.codec,
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
        isPlaying: tocando,
        technicalLine: tecnica
    )
}
