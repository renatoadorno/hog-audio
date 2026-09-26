import Foundation
import UniformTypeIdentifiers

/// Expande pastas recursivamente, descarta itens que não são áudio e ordena por caminho.
public func audioFiles(from urls: [URL]) -> [URL] {
    let fileManager = FileManager.default
    var result: [URL] = []

    for url in urls {
        var isDirectory: ObjCBool = false
        guard fileManager.fileExists(atPath: url.path, isDirectory: &isDirectory) else {
            continue
        }
        if isDirectory.boolValue {
            guard let enumerator = fileManager.enumerator(
                at: url,
                includingPropertiesForKeys: [.isRegularFileKey, .contentTypeKey],
                options: [.skipsHiddenFiles]
            ) else { continue }
            for case let child as URL in enumerator {
                if isAudioFile(child) { result.append(child) }
            }
        } else if isAudioFile(url) {
            result.append(url)
        }
    }

    return result.sorted { $0.path < $1.path }
}

private func isAudioFile(_ url: URL) -> Bool {
    guard let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .contentTypeKey]),
          values.isRegularFile == true,
          let type = values.contentType else {
        return false
    }
    return type.conforms(to: .audio)
}
