import AVFoundation
import Foundation

public struct TrackMetadata: Equatable, Sendable {
    public let title: String
    public let artist: String?
    public let album: String?
    public let artwork: Data?
}

/// Um item de metadado já extraído do AVFoundation. A separação existe para que a regra de
/// mapeamento seja testável sem arquivo nenhum.
public struct MetadataItem: Sendable {
    public let keySpace: String
    public let key: String
    public let stringValue: String?
    public let dataValue: Data?

    public init(keySpace: String, key: String, stringValue: String?, dataValue: Data?) {
        self.keySpace = keySpace
        self.key = key
        self.stringValue = stringValue
        self.dataValue = dataValue
    }
}

private let titleKeys: Set<String> = ["title", "TITLE", "TIT2"]
private let artistKeys: Set<String> = ["artist", "ARTIST", "TPE1"]
private let albumKeys: Set<String> = ["albumName", "ALBUM", "TALB"]
private let artworkKeys: Set<String> = ["artwork", "METADATA_BLOCK_PICTURE", "APIC"]

private func firstText(_ items: [MetadataItem], _ keys: Set<String>) -> String? {
    for item in items where keys.contains(item.key) {
        if let value = item.stringValue?.trimmingCharacters(in: .whitespacesAndNewlines),
           !value.isEmpty {
            return value
        }
    }
    return nil
}

private func firstData(_ items: [MetadataItem], _ keys: Set<String>) -> Data? {
    for item in items where keys.contains(item.key) {
        if let data = item.dataValue, !data.isEmpty { return data }
    }
    return nil
}

/// O FLAC devolve `commonMetadata` vazio — medido —, então a lista crua é o caminho
/// alternativo. Nos demais formatos o comum já traz tudo, capa inclusa.
public func trackMetadata(
    common: [MetadataItem],
    raw: [MetadataItem],
    fallbackFilename: String
) -> TrackMetadata {
    let todos = common + raw
    return TrackMetadata(
        title: firstText(todos, titleKeys) ?? fallbackFilename,
        artist: firstText(todos, artistKeys),
        album: firstText(todos, albumKeys),
        artwork: firstData(todos, artworkKeys)
    )
}

/// Lê os metadados de um arquivo. Nunca falha: sem tags legíveis, o título vira o nome do
/// arquivo.
public func loadMetadata(from url: URL) async -> TrackMetadata {
    let asset = AVURLAsset(url: url)
    let nome = url.deletingPathExtension().lastPathComponent

    func converter(_ items: [AVMetadataItem]) async -> [MetadataItem] {
        var resultado: [MetadataItem] = []
        for item in items {
            let chave = item.commonKey?.rawValue
                ?? item.key.map { "\($0)" }
                ?? item.identifier?.rawValue
                ?? ""
            let texto = try? await item.load(.stringValue)
            let dados = try? await item.load(.dataValue)
            resultado.append(MetadataItem(
                keySpace: item.keySpace?.rawValue ?? "",
                key: chave,
                stringValue: texto ?? nil,
                dataValue: dados ?? nil
            ))
        }
        return resultado
    }

    guard let common = try? await asset.load(.commonMetadata),
          let raw = try? await asset.load(.metadata) else {
        return TrackMetadata(title: nome, artist: nil, album: nil, artwork: nil)
    }

    return trackMetadata(
        common: await converter(common),
        raw: await converter(raw),
        fallbackFilename: nome
    )
}
