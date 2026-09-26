import Foundation

/// A família 44,1/88,2/176,4 kHz (derivada de CD) tem casa decimal real; 48/96/192 são
/// múltiplos redondos de 1000 e não têm. Suprimir a casa quando ela é zero evita "96.0 kHz"
/// sem esconder "44.1 kHz" atrás de um arredondamento que mostraria "44 kHz" — informação
/// errada num projeto que existe para ser transparente sobre o que chega ao DAC.
private func formatKilohertz(_ hz: Double) -> String {
    let khz = hz / 1000
    if khz.truncatingRemainder(dividingBy: 1) == 0 {
        return String(format: "%.0f", khz)
    }
    return String(format: "%.1f", khz)
}

/// "44.1 kHz · 24 bit · flac": a linha técnica sob a capa e o tooltip do selo da fila saem
/// daqui, para as duas nunca descreverem a mesma faixa de jeitos diferentes.
public func qualityDetail(sampleRate: Double, bitDepth: UInt32, codec: String) -> String {
    "\(formatKilohertz(sampleRate)) kHz · \(bitDepth) bit · \(codec)"
}

/// "24/44.1": o selo compacto da fila, na convenção bits/kHz das lojas de hi-res.
public func compactQuality(sampleRate: Double, bitDepth: UInt32) -> String {
    "\(bitDepth)/\(formatKilohertz(sampleRate))"
}
