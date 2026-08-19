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
    // `keySpace` é preservado mas o casamento abaixo usa só `key`: hoje não há colisão de
    // nome entre `vorb`/`org.id3`/`itsk` (ver as tabelas de chaves logo abaixo), então
    // qualificar por keySpace só complicaria a concatenação comum+cru. Se um dia duas
    // keyspaces usarem a mesma chave para campos diferentes, isso vira um bug silencioso —
    // é o preço combinado desta simplicidade.
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
    let combined = common + raw
    return TrackMetadata(
        title: firstText(combined, titleKeys) ?? fallbackFilename,
        artist: firstText(combined, artistKeys),
        album: firstText(combined, albumKeys),
        artwork: firstData(combined, artworkKeys)
    )
}

/// Lê os metadados de um arquivo. Nunca falha: sem tags legíveis, o título vira o nome do
/// arquivo.
public func loadMetadata(from url: URL) async -> TrackMetadata {
    let asset = AVURLAsset(url: url)
    let name = url.deletingPathExtension().lastPathComponent

    func convert(_ items: [AVMetadataItem]) async -> [MetadataItem] {
        var result: [MetadataItem] = []
        for item in items {
            let rawKey = item.commonKey?.rawValue
                ?? item.key.map { "\($0)" }
                ?? item.identifier?.rawValue
                ?? ""
            let text = try? await item.load(.stringValue)
            let data = try? await item.load(.dataValue)
            result.append(MetadataItem(
                keySpace: item.keySpace?.rawValue ?? "",
                key: rawKey,
                stringValue: text ?? nil,
                dataValue: data ?? nil
            ))
        }
        return result
    }

    guard let common = try? await asset.load(.commonMetadata),
          let raw = try? await asset.load(.metadata) else {
        return TrackMetadata(title: name, artist: nil, album: nil, artwork: nil)
    }

    return trackMetadata(
        common: await convert(common),
        raw: await convert(raw),
        fallbackFilename: name
    )
}
