import Testing
import HogAudioBindings

@Test func playerNovoComecaEmIdle() {
    let player = HogPlayer()
    let snapshot = player.snapshot()
    #expect(snapshot.state == .idle)
    #expect(snapshot.elapsedSeconds == 0)
    #expect(snapshot.underruns == 0)
}

@Test func arquivoInexistenteLancaErroTipado() {
    let player = HogPlayer()
    #expect(throws: PlayerError.self) {
        _ = try player.load(path: "/tmp/nao-existe-mesmo-12345.flac")
    }
}

@Test func aInterpretacaoDeVolumeAtravessaAFronteira() {
    // A regra vive no Rust; a interface nao pode ter uma segunda copia dela.
    #expect(parseVolumeText(text: "35") == 0.35)
    #expect(parseVolumeText(text: "35%") == 0.35)
    #expect(parseVolumeText(text: "35x") == nil)
}
