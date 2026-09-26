import CoreGraphics
import Foundation
import HogAudioBindings
import ImageIO

/// O que uma linha da fila mostra. Chega depois que a faixa entra na fila; até lá a linha usa
/// o nome do arquivo.
public struct QueueEntryInfo: Equatable, Sendable {
    public let title: String
    public let artist: String?
    public let album: String?
    public let quality: String?
    public let qualityDetail: String?
    public let duration: String?
    public let thumbnail: CGImage?
    public let problem: String?

    public var subtitle: String {
        [artist, album].compactMap { $0 }.joined(separator: " — ")
    }
}

/// O resultado da sondagem do formato, já em tipo `Sendable` para atravessar de volta da
/// thread onde a chamada bloqueante ao Rust rodou.
public enum ProbeOutcome: Sendable {
    case probed(TrackProbe)
    case failed(String)
}

/// 44 pt na tela, em Retina: 88 px, com folga. A lista guarda só isto — a capa embutida
/// inteira tem de 1 a 5 MB, e uma fila de mil faixas não pode segurar mil delas.
let queueThumbnailPixels = 96

public func queueEntryInfo(
    metadata: TrackMetadata,
    probe: ProbeOutcome,
    thumbnail: CGImage?
) -> QueueEntryInfo {
    switch probe {
    case .probed(let format):
        return QueueEntryInfo(
            title: metadata.title,
            artist: metadata.artist,
            album: metadata.album,
            quality: compactQuality(sampleRate: format.sampleRate, bitDepth: format.bitDepth),
            qualityDetail: qualityDetail(
                sampleRate: format.sampleRate,
                bitDepth: format.bitDepth,
                codec: format.codec
            ),
            duration: formatTime(format.totalSeconds),
            thumbnail: thumbnail,
            problem: nil
        )
    case .failed(let reason):
        return QueueEntryInfo(
            title: metadata.title,
            artist: metadata.artist,
            album: metadata.album,
            quality: nil,
            qualityDetail: nil,
            duration: nil,
            thumbnail: thumbnail,
            problem: reason
        )
    }
}

/// Reduz sem decodificar a imagem inteira, e já decodificada: o SwiftUI desenha a miniatura
/// sem trabalho na main thread.
public func makeThumbnail(from data: Data, maxPixelSize: Int) -> CGImage? {
    guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
    let options: [CFString: Any] = [
        kCGImageSourceCreateThumbnailFromImageAlways: true,
        kCGImageSourceCreateThumbnailWithTransform: true,
        kCGImageSourceThumbnailMaxPixelSize: maxPixelSize,
        kCGImageSourceShouldCacheImmediately: true,
    ]
    return CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary)
}

/// Tags e capa pelo AVFoundation, formato pelo mesmo `AudioSource` que o engine usa — o selo
/// mostra o que vai chegar ao DAC, não o que a extensão do arquivo sugere.
public func loadQueueEntry(path: String) async -> QueueEntryInfo {
    let metadata = await loadMetadata(from: URL(fileURLWithPath: path))
    let probe = await Task.detached { () -> ProbeOutcome in
        do {
            return .probed(try probeTrack(path: path))
        } catch let error as PlayerError {
            return .failed(message(of: error))
        } catch {
            return .failed("\(error)")
        }
    }.value
    let thumbnail = metadata.artwork.flatMap {
        makeThumbnail(from: $0, maxPixelSize: queueThumbnailPixels)
    }
    return queueEntryInfo(metadata: metadata, probe: probe, thumbnail: thumbnail)
}

/// O texto que o Rust escreveu. Interpolar o erro direto daria `Load(message: "...")`, a
/// descrição do enum que o uniffi gera.
private func message(of error: PlayerError) -> String {
    switch error {
    case .Load(let message), .Device(let message), .State(let message):
        return message
    }
}
