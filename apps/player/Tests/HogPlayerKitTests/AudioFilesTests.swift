import Foundation
import Testing
@testable import HogPlayerKit

@Test func expandePastasAninhadasFiltraEOrdenaAudio() throws {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent("hog-audio-ingestion-\(UUID().uuidString)", isDirectory: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let nested = root.appendingPathComponent("CD2", isDirectory: true)
    try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)

    let paths = [
        root.appendingPathComponent("02-song.wav"),
        root.appendingPathComponent("cover.jpg"),
        nested.appendingPathComponent("01-song.flac"),
        nested.appendingPathComponent("notes.txt"),
    ]
    for url in paths { FileManager.default.createFile(atPath: url.path, contents: Data()) }

    let result = audioFiles(from: [root])

    #expect(result.map(\.lastPathComponent) == ["02-song.wav", "01-song.flac"])
}

@Test func listaVaziaOuSemAudioDevolveVazio() throws {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent("hog-audio-no-audio-\(UUID().uuidString)", isDirectory: true)
    defer { try? FileManager.default.removeItem(at: root) }
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    FileManager.default.createFile(
        atPath: root.appendingPathComponent("cover.png").path,
        contents: Data()
    )

    #expect(audioFiles(from: []).isEmpty)
    #expect(audioFiles(from: [root]).isEmpty)
}
