import Foundation
import Testing
import UniformTypeIdentifiers

/// O Finder só oferece o HogAudio em "Abrir com" se o bundle declarar que abre áudio. Lido do
/// arquivo que o `make app` copia para o bundle, a partir de `apps/player`, de onde o
/// `swift test` roda.
private func audioDocumentType() throws -> [String: Any] {
    let data = try Data(contentsOf: URL(fileURLWithPath: "Resources/Info.plist"))
    let plist = try #require(
        try PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any]
    )
    let types = try #require(plist["CFBundleDocumentTypes"] as? [[String: Any]])
    return try #require(types.first {
        ($0["LSItemContentTypes"] as? [String])?.contains("public.audio") == true
    })
}

@Test func oAppDeclaraQueAbreArquivosDeAudioSemTomarOPadrao() throws {
    let audio = try audioDocumentType()
    #expect(audio["CFBundleTypeRole"] as? String == "Viewer")
    // "Alternate": aparece em "Abrir com" sem disputar o padrão com o Música e o QuickTime.
    #expect(audio["LSHandlerRank"] as? String == "Alternate")
    #expect(Set(audio["LSItemContentTypes"] as? [String] ?? []) == [
        "public.audio",
        "public.mp3",
        "public.mpeg-4-audio",
        "com.apple.m4a-audio",
        "org.xiph.flac",
        "com.microsoft.waveform-audio",
        "public.aiff-audio",
        "public.aifc-audio",
        "com.apple.coreaudio-format",
    ])
}

// Um UTI digitado errado não dá erro: o macOS só deixa de casar o tipo, e o app some do
// "Abrir com" daquele formato.
@Test func todoTipoDeclaradoEUmTipoDeAudioConhecido() throws {
    let declared = try #require(try audioDocumentType()["LSItemContentTypes"] as? [String])
    for identifier in declared {
        let type = try #require(UTType(identifier), "UTI desconhecido: \(identifier)")
        #expect(type.conforms(to: .audio), "\(identifier) não é áudio")
    }
}
