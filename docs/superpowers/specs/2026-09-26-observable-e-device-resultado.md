# Observable, fila com identidade e robustez do device: resultado

Data: 2026-09-26
Plano: `~/.claude/plans/o-seguinte-estou-sorted-lighthouse.md`
Estado: no working tree da branch `feat/observable-e-device`, sem commit.

## O que mudou

**SwiftUI.**
- O ViewModel é `@Observable`: cada tela recalcula só quando muda algo que ela leu.
- O relógio saiu do `DisplayState` para um `ClockState` próprio. Um tique tocando não
  recalcula mais capa, título e fila.
- `ClockView`, `VolumeControl` e `QueuePanel` viraram structs próprias.
- O timer do poll tem 20 ms de tolerância.

**Fila.**
- Cada entrada tem um `id: u64` que nunca é reaproveitado, nem depois de limpar.
- Remoção e seleção são por id, resolvidas sob o lock de comandos.
- Remover a faixa atual devolve o formato de quem assumiu. Antes, o Swift carregava essa faixa
  de novo: tocando, eram duas aquisições do device e um corte no som.

**Listeners** (`device_watch.rs`).
- Acompanham vivo, hog, rate, streams, volume, mudo, formato físico e a lista de devices.
- O callback só soma gerações e acorda quem espera; nunca chama o HAL.
- O registro é permanente, por id mais UID. Nada é removido, porque o `AudioHardware.h` não
  garante que a remoção espere um callback em andamento.

**Recuperação** (`recovery.rs`).
- **Veredito puro:** o device atual é comparado com a linha de base lida do HAL no fim do
  `acquire`, e o resultado é `Disconnected`, `HogLost` ou `FormatChanged`.
- **Poll e comando separados:** o poll só marca a suspeita, e só lê o device quando um listener
  de perda avisa ou quando os frames param por mais de 500 ms tocando. `recover()` confirma e
  faz o teardown.
- **Onde a perda para tudo:** a carga confere o device retido ao vivo, e perdido vira erro de
  device. Isso cobre auto-avanço, navegação, seleção e remoção.
- **Só o Play explícito renegocia** com o device padrão. A recusa é do device, não da faixa.
- **Pause:** retomar revalida o device, o que cobre o Mac dormir pausado.
- **Hog ocupado:** detectado antes de qualquer escrita, com a mensagem "em uso exclusivo por
  <App> (pid N)".
- **Restore:** só escreve em device vivo e ainda nosso. O IOProc é destruído antes de o
  `Arc<Playback>` cair.
- **Volume por device:** o pedido fica preso ao `DeviceUID`. Num device de UID diferente, vale
  o teto, com aviso.

**Volume externo e mudo.**
- O `Snapshot` traz `volume_scalar: Option<f32>` e `muted`.
- Uma mudança externa vira o pedido da próxima faixa.
- O slider segue o volume do device só em Playing ou Paused, sem arrasto, sem escrita a caminho
  e sem comando na fila.

**Tempo de volume.**
- A escrita de volume continua logo depois do formato.
- A conferência (os 30 ms que dão à leitura o poder de discordar) passou para depois do
  prefill, antes do start: a espera corre junto com o trabalho da partida.
- O `wait_for_rate` e a nova espera do formato físico acordam na notificação, com o poll de
  10 ms como reserva.

## Decisões da rodada

- **SleepPreventer descartado.** Tocando em hog pela CLI, o `coreaudiod` segurou
  `PreventUserIdleSystemSleep` "Created for PID" do hog-audio (`audio-out
  BuiltInHeadphoneOutputDevice`), e soltou ao terminar.
- **Confirmação de volume por listener (6b) não adotada.**
  - O experimento (`rust/tests/volume_notification.rs`) fez 400 escritas no momento vulnerável:
    200 logo depois do `acquire` e 200 logo depois de devolver rate e formato. Nenhuma foi
    descartada; a notificação chegou em 4,3 ms de mediana, 21,6 ms no máximo.
  - Sem descarte, o experimento não diz se uma escrita descartada é notificada. A espera de
    30 ms fica, escondida atrás do prefill.
- **Run loop do HAL.** O `AudioHardwareDeprecated.h` diz que, desde o 10.6, as notificações
  saem pelo run loop principal. No macOS 27 o `kAudioHardwarePropertyRunLoop` já vem NULL
  (thread própria do HAL), e o listener disparou num teste sem run loop. Um teste reprova se
  isso mudar.

## Bit-perfect

Nada tocou IOProc, ring, decodificador ou negociação. A ordem hog → rate → formato → volume
não mudou; o que mudou foram esperas e leituras dentro dela. Provas depois de tudo:
- `make verify` em 24/96: 1.152.000 amostras idênticas ao oráculo ffmpeg;
- `make verify` em 16/44.1: 529.200 amostras idênticas;
- `make verify-pause`: fluxo idêntico com e sem pausa;
- `make verify-volume`: amostras idênticas a 20% e 90%.

## Medições

Mesmos trechos de 26/09 (24/96, 24/96, 24/44.1, 8 s cada), saída embutida, volume pedido de
29%. Três rodadas pelo app; entre parênteses, a linha de base de 26/09.
- **play:** 90, 86 e 89 ms (98). O volume caiu de 36 ms para 11 a 21 ms.
- **auto-avanço no mesmo formato:** 143, 110 e 114 ms (122). O volume ficou entre 1 e 19 ms.
  A primeira rodada teve um prefill de 57 ms.
- **troca de formato:** 101, 89 e 103 ms (107). O volume ficou entre 17 e 28 ms, porque um
  prefill curto esconde menos.

O `release` subiu de cerca de 51 para cerca de 55 ms: são as leituras novas (precondição do
`restore`, conferência do volume antes de soltar) mais o ruído. O ganho líquido fica em 6 a
10 ms por transição.

## Pendências

- **Teste manual:** desconectar o fone tocando e no fim de uma faixa.
- **Três achados da revisão deixados de fora de propósito:**
  - leitura transitória virando perda: o `restore` e o `recover` já releem;
  - slider mexido depois da perda valendo no próximo device: é escolha explícita do usuário;
  - registro parcial de listeners: gravá-lo esconderia a falha.
- **Já existiam antes desta rodada:**
  - o Delete da fila só funciona com a lista em foco; depois de mexer no slider, é preciso
    clicar na lista;
  - com "nenhuma faixa carregada", o relógio ainda mostra a duração da faixa removida.

## Testes

- 211 testes Rust (+59), com 17 de hardware ignorados (+3) e o experimento 6b.
- 87 testes Swift (+19).
- Todos os testes novos foram vistos falhando antes de passar, contra stub ou com o código
  sabotado. Uma sabotagem que não pegou mostrou que as guardas extras do auto-avanço eram
  redundantes, e elas saíram.
- Uma revisão adversarial do diff achou quatro problemas médios, todos corrigidos com teste
  vermelho antes:
  - a linha de base podia ser lida antes de o formato assentar;
  - remover a atual com o device perdido tocaria a faixa removida;
  - retomar do Pause podia deixar o player pausado sem device;
  - o aviso de volume podia sumir da tela.
- `make hw-test` (18 testes) passou, incluindo a linha de base conferida 500 ms depois do
  `acquire`.
