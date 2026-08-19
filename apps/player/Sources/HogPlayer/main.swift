import HogAudioBindings

// Provisório: a Task 13 troca isto pela janela SwiftUI. Serve para o alvo executável ter
// ponto de entrada e para provar a linkagem fora do contexto de teste.
let player = HogPlayer()
print("estado inicial: \(player.snapshot().state)")
