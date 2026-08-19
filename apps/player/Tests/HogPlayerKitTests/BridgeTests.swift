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
    do {
        _ = try player.load(path: "/tmp/nao-existe-mesmo-12345.flac")
        Issue.record("deveria ter lançado")
    } catch let erro as PlayerError {
        guard case .Load = erro else {
            Issue.record("esperava .Load, veio \(erro)")
            return
        }
    } catch {
        Issue.record("esperava PlayerError, veio \(error)")
    }
}

@Test func aInterpretacaoDeVolumeAtravessaAFronteira() {
    // A regra vive no Rust; a interface não pode ter uma segunda cópia dela.
    #expect(parseVolumeText(text: "35") == 0.35)
    #expect(parseVolumeText(text: "35%") == 0.35)
    #expect(parseVolumeText(text: "35x") == nil)
}
