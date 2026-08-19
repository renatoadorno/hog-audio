import Foundation

/// Relógio da faixa. Valores inválidos — negativos, NaN, infinito — viram `0:00` em vez de
/// texto quebrado na tela: `totalSeconds` chega zerado antes de qualquer carga, e uma taxa de
/// amostragem zerada produz NaN.
public func formatTime(_ seconds: Double) -> String {
    guard seconds.isFinite, seconds > 0 else { return "0:00" }
    let total = Int(seconds.rounded(.down))
    let horas = total / 3600
    let minutos = (total % 3600) / 60
    let segundos = total % 60
    if horas > 0 {
        return String(format: "%d:%02d:%02d", horas, minutos, segundos)
    }
    return String(format: "%d:%02d", minutos, segundos)
}
