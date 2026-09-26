import CoreGraphics

/// A conta de tamanho da janela num lugar só. A coluna da esquerda é quadrada na capa e tem
/// altura fixa nos controles abaixo dela; a fila, à direita, fica com o que sobrar. Função pura
/// pelo mesmo motivo de `displayState`: dá para provar o layout sem abrir janela.
public enum PlayerLayout {
    public static let padding: CGFloat = 24
    public static let columnSpacing: CGFloat = 24
    /// Altura do bloco abaixo da capa — título, relógio, transporte, volume, linha técnica.
    /// Todas as linhas desse bloco são de uma linha só, então a altura não depende da largura.
    /// Medido na janela real (261 pt com artista, álbum e linha técnica), mais folga de 3.
    public static let controlsHeight: CGFloat = 264
    public static let minQueueWidth: CGFloat = 340
    public static let coverRange: ClosedRange<CGFloat> = 320...560
    public static let minWindow = CGSize(width: 820, height: 640)

    /// Lado da capa: cresce com a altura, sem roubar a largura mínima da fila.
    public static func coverSide(in size: CGSize) -> CGFloat {
        let byHeight = size.height - 2 * padding - controlsHeight
        let byWidth = size.width - 2 * padding - columnSpacing - minQueueWidth
        return min(max(min(byHeight, byWidth), coverRange.lowerBound), coverRange.upperBound)
    }
}
