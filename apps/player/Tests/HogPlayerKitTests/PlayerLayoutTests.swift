import CoreGraphics
import Testing
@testable import HogPlayerKit

private func queueWidth(in size: CGSize, cover: CGFloat) -> CGFloat {
    size.width - 2 * PlayerLayout.padding - PlayerLayout.columnSpacing - cover
}

@Test func noTamanhoMinimoCapaEFilaCabem() {
    let size = PlayerLayout.minWindow
    let cover = PlayerLayout.coverSide(in: size)
    #expect(cover >= PlayerLayout.coverRange.lowerBound)
    #expect(queueWidth(in: size, cover: cover) >= PlayerLayout.minQueueWidth)
    // A coluna da esquerda também tem de caber na altura: capa mais controles.
    #expect(cover + PlayerLayout.controlsHeight + 2 * PlayerLayout.padding <= size.height)
}

@Test func comLarguraSobrandoACapaCresceComAAltura() {
    let baixa = PlayerLayout.coverSide(in: CGSize(width: 1400, height: 700))
    let alta = PlayerLayout.coverSide(in: CGSize(width: 1400, height: 800))
    #expect(alta - baixa == 100)
}

@Test func larguraCurtaEncolheACapaParaPreservarAFila() {
    // Altura sobrando: quem manda é a largura, e a fila fica exatamente no mínimo.
    let size = CGSize(width: 820, height: 1000)
    let cover = PlayerLayout.coverSide(in: size)
    #expect(queueWidth(in: size, cover: cover) == PlayerLayout.minQueueWidth)
}

@Test func capaNuncaPassaDoTeto() {
    let cover = PlayerLayout.coverSide(in: CGSize(width: 3000, height: 3000))
    #expect(cover == PlayerLayout.coverRange.upperBound)
}
