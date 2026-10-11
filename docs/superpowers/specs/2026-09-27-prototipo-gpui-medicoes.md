# Medições do protótipo GPUI (11/10/2026)

Números brutos da Task 13 do plano `docs/superpowers/plans/2026-09-27-prototipo-gpui.md`.
A leitura e a conclusão ficam no resultado (Task 14).

## Montagem

- Máquina: MacBook Air M1 (8 núcleos), 16 GB, macOS 27.0.1, na tomada.
- Escopo combinado com o usuário: CPU e RAM. Sem energia e sem `sudo`: o `powermetrics` e o
  `purge` saem do protocolo, e com eles a potência da GPU e a abertura a frio.
- O custo do app parado vem em destaque, em dois estados: aberto vazio, e com a biblioteca na
  fila e uma faixa pausada.
- Controle: `apps/player/HogAudio.app`, montado por `make app` nesta branch logo depois do build
  limpo. O `rust/` e o `apps/player/` são idênticos ao `main`. O binário em `/Applications` era
  anterior aos últimos commits de código do `main` e não foi usado.
- Protótipo: `apps/gpui/build/HogAudioGPUI.app`, montado por `make gpui-app` no commit 202fa01.
- Um app aberto por vez. Sem macos-use ativo durante as medições: ele liga a acessibilidade, que
  no protótipo tira o cache das views até o app fechar, e atrasa a entrega de cliques sintéticos.
- CPU: `top` amostrado a cada 1 s, sem a 1ª amostra; mediana, média e máximo por rodada.
  Memória: `footprint`.

## Biblioteca

`/Users/renatoadorno/Music/Masterizados`: 221 faixas (210 FLAC e 11 de outros formatos), 11 GB,
todas locais (nenhuma `dataless`). O plano previa 1.157 faixas. Com 221, o custo de uma fila
muito grande aparece só em parte.

## Ressalvas da montagem

- As janelas não tinham o mesmo tamanho: o protótipo abre sempre em 920×740; o controle
  restaurou 980×692 da última sessão dele. O ajuste a 1280×800 do plano exigiria o macos-use,
  que liga a acessibilidade e tira o cache do protótipo; ficou de fora para manter as medições
  sem acessibilidade.
- Nas duas rodadas do controle da medição 2, o clique que baixaria o volume para 10% caiu fora
  do slider (a conta usava o layout de 920×740), e a faixa tocou no teto de 50%. Não muda o
  custo medido.

## Medição 1: custo parado

A pedido do usuário, janelas de 20 s (não 60) e duas rodadas por app, em ordem alternada.
Cada linha traz a mediana, a média e o máximo de CPU (%) e o footprint.

### App aberto vazio, sem fazer nada

- Protótipo, rodada 1: na frente 0,70 / 0,78 / 1,10, 32 MB; minimizado 0,25 / 1,88 / 9,30,
  33 MB.
- Protótipo, rodada 2: na frente 0,80 / 0,86 / 1,20, 33 MB; minimizado 0,20 / 0,69 / 4,50,
  32 MB.
- Controle, rodada 1: na frente 0,20 / 0,24 / 0,80, 32 MB; minimizado 0,20 / 0,26 / 0,80,
  36 MB.
- Controle, rodada 2: na frente 0,20 / 0,34 / 1,60, 31 MB; minimizado 0,20 / 0,25 / 0,80,
  35 MB.

### Biblioteca na fila e uma faixa pausada

O app abre com a pasta como argumento; o roteiro toca e pausa logo (o device fica tomado, em
pausa).

- Protótipo, rodada 1: na frente 1,45 / 1,48 / 2,10, 117 MB; minimizado 0,60 / 0,63 / 0,80,
  81 MB.
- Protótipo, rodada 2: na frente 1,50 / 3,16 / 20,10, 101 MB; minimizado 1,35 / 2,16 / 7,40,
  82 MB.
- Controle, rodada 1: na frente 0,60 / 0,58 / 1,00, 79 MB; minimizado 0,70 / 0,74 / 1,30,
  87 MB.
- Controle, rodada 2: na frente 0,70 / 1,89 / 9,80, 82 MB; minimizado 0,70 / 0,72 / 1,20,
  97 MB.

### Leitura rápida

- Vazio, na frente: o protótipo gasta 0,7 a 0,8% de CPU (mediana) contra 0,2% do controle,
  cerca de 4 vezes. A memória é a mesma (31 a 33 MB).
- Com a fila e a faixa pausada, na frente: 1,45 a 1,50% contra 0,6 a 0,7%, cerca de 2 vezes.
- Minimizado, os dois caem; o protótipo fica entre 0,2 e 1,35%, o controle em 0,2 a 0,7%.

## Medição 2: tocando

Primeira faixa da biblioteca tocando, janela na frente, 20 s por rodada, duas rodadas por app.
Mediana, média e máximo de CPU (%) e footprint.

- Protótipo, rodada 1: 6,15 / 7,78 / 14,70, 98 MB.
- Protótipo, rodada 2: 6,05 / 7,19 / 13,90, 86 MB.
- Controle, rodada 1: 6,25 / 6,26 / 7,40, 81 MB.
- Controle, rodada 2: 6,55 / 6,73 / 11,60, 80 MB.

Tocando, a mediana empata (cerca de 6%): o custo é dominado pelo core (decodificação e
IOProc), que é o mesmo. O protótipo tem média e picos maiores e uns 10 a 15 MB a mais.

## Medição 3: memória

Footprint nos três momentos do protocolo, com a biblioteca de 221 faixas.

- Logo depois de abrir, vazio: protótipo 32 a 33 MB; controle 31 a 32 MB.
- Com a fila carregada: protótipo 117 e 122 MB; controle 78 e 82 MB. Os dois apps leem todas
  as linhas da fila, não só as visíveis: na ordem da fila, quatro por vez (`EntryLoader::sync`
  em `apps/gpui/model/src/controller/loaders.rs`; `pendingEntryPaths` com
  `maxConcurrentEntryLoads` em `apps/player/Sources/HogPlayerKit/PlayerViewModel.swift`).
- Depois de rolar a fila inteira (`input_probe scroll`, 400 passos de -5 linhas): protótipo 85 e
  82 MB; controle 89 e 93 MB.
- Minimizado depois da rolagem: protótipo 81 e 82 MB; controle 87 e 97 MB.

O protótipo começa mais alto com a fila (cerca de 40 MB a mais) e cai depois da rolagem; o
controle sobe ao rolar. Depois de percorrer a fila os dois ficam na mesma faixa (80 a 97 MB).

Causa provável dos 40 MB a mais, lida no código e não medida: cada linha do protótipo
decodifica a capa embutida inteira (`image::load_from_memory` em
`apps/gpui/model/src/media/artwork.rs`) e só depois a reduz a 96 px, com quatro linhas
carregando ao mesmo tempo. O controle pede só a miniatura ao ImageIO
(`CGImageSourceCreateThumbnailAtIndex` com `kCGImageSourceThumbnailMaxPixelSize` de 96, em
`apps/player/Sources/HogPlayerKit/QueueEntry.swift`); o comentário de `makeThumbnail` diz que ela
sai sem decodificar a imagem inteira. As miniaturas guardadas têm o mesmo lado máximo nos dois
apps (96 px): a diferença estaria no pico da carga.

O tempo de "encher a fila" não se mediu nesta rodada. O critério do plano é o tempo até a CPU
ficar abaixo de 2% por 3 s, e ela já estava abaixo de 2% no primeiro segundo amostrado: não
houve trecho para medir. Quatro cargas decodificando capas dificilmente caberiam em 2%. A
explicação provável é que a carga das 221 linhas terminou antes da primeira amostra que conta
(a 1ª do `top` é descartada). Não foi conferido.

## Medição 4: abertura

Quente, cinco rodadas seguidas por app, com o `window_timer` (do lançamento até a janela no
WindowServer). A fria saiu do escopo (exige `sudo purge`).

- Protótipo: 193, 207, 216, 216 e 223 ms; mediana 216 ms.
- Controle: 267, 269, 264, 251 e 257 ms; mediana 264 ms.

## Medição 5: resposta (parcial)

Fora do escopo combinado (CPU e RAM). O que o uso real da Task 11 já mediu, pelo log de
transições do protótipo:

- Troca de faixa tocando (seleção, Próxima, auto-avanço): comandos de 41 a 135 ms, quase todo o
  tempo no core (release, acquire, volume e start).
- Três cliques rápidos em Próxima: viram "navegação 1" e "navegação 2" (a segunda espera a
  primeira e soma os passos), sem o macos-use ativo. Com o macos-use ativo, os cliques sintéticos
  chegam atrasados de fora do app e viram três navegações de um passo.
- Delete de uma linha que não toca: "remoção" de 0 ms, sem corte na faixa atual.
- Não medido: o tempo do slider até o hardware (`input_probe slider`), o clique na fila
  (`input_probe click`) e os hitches de rolagem com o Instruments, nos dois apps.

## Medição 6: áudio sob estresse (versão curta)

Só no protótipo, que é o que traz a tecnologia nova. 60 s tocando uma faixa 24/44.1 da
biblioteca no fone, com volume em 13%. Em laço: rolagem da fila (`input_probe scroll`, 20 linhas
para baixo e para cima) e arrasto do slider entre 5 e 15%; 36 gestos no total. O device foi
conferido depois de cada gesto (hog do app).

- Tocou os 60 s sem parar. A linha técnica terminou em "44.1 kHz · 24 bit · flac · hog ativo",
  sem "underruns".
- Sem redimensionamento (exigiria o macos-use ou recalcular as posições a cada tamanho) e sem
  `Audio System Trace`: versão curta a pedido do usuário.
- Na primeira tentativa a reprodução parou entre o play e o primeiro gesto; o usuário confirmou
  que foi ação dele durante o teste. A tentativa foi descartada. Na segunda, com o device
  conferido a cada gesto, tocou até o fim.

## Medição 7: VoiceOver (parcial)

Não feita com o VoiceOver ligado. A árvore de acessibilidade do protótipo foi conferida pelo
macos-use (Tasks 10 e 11): botões, slider, fila com rótulo por linha, capa, faixa atual, relógio
e linha técnica. Ela segue completa tocando, depois da correção que tira o cache das views com a
acessibilidade ativa. Custo dessa correção: no macOS a ativação não desliga até o app fechar, e
a janela fica sem cache enquanto isso.

## Medição 8: integração com o Mac (protótipo)

Do uso real da Task 11, só no protótipo:

- ⌘O no menu e como atalho: sim. O painel de abrir aceita arquivos e pastas, sem filtro de tipo.
- Arrastar do Finder: coberto por teste (evento de soltar simulado); não exercitado à mão.
- Barra de título: só os 28 pt de cima arrastam a janela; a faixa de 40 pt do botão Abrir abaixo
  disso não arrasta.
- Delete e espaço: sim.
- ⌘Q, Ctrl+C, SIGTERM, menu Fechar e botão vermelho tocando: devolvem rate, volume e hog
  (lidos pelo `input_probe`).
- O lado do controle não foi medido nesta rodada.

## Medição 9: pânico

Protótipo compilado com `--features panic-probe`, rodado pelo terminal com
`testdata/t44_longo.flac` tocando no fone (o device passou de 96 kHz para 44,1 kHz), e o menu
Depurar › Provocar pânico.

- O processo abortou: "thread caused non-unwinding panic. aborting.", código 134. O pânico num
  handler de ação vira abort porque o callback do AppKit é `extern "C"`.
- O stderr mostrou "panicked at app/src/actions.rs:61:40: pânico provocado pela medição 9" e,
  antes do abort, "pânico: device devolvido". A mesma linha foi para
  `~/Library/Logs/hog-player-gpui.log`.
- `input_probe read` logo depois: volume 0,625, 96000 Hz, hog -1. Rate, volume e dono voltaram
  ao estado de antes.
- O app foi recompilado sem a feature (o binário não tem mais o item "Provocar pânico").

## Medição 10: build

### Protótipo

- `cargo clean` em `apps/gpui` (20,1 GiB removidos) e `cargo build --locked --release -p
  hog-player-gpui`: 4 min 11 s de relógio (real 251,3 s; user 930,5 s; sys 53,5 s).
- 397 crates compiladas; 707 pacotes no `Cargo.lock`.
- `.app` de 14 MB (binário de 14,8 MB). Target release depois do build: 1,2 GB.
- Exige o Xcode completo: o GPUI compila os shaders com o Metal Toolchain no `build.rs`.

### Controle

- `cargo clean` em `rust/` (4,3 GiB removidos), `swift package reset` em `apps/player` e `make
  app`: 1 min 51 s de relógio (real 110,8 s; user 322,5 s; sys 20,9 s). O core em release levou
  1 min 34 s e o `swift build -c release`, 14,2 s.
- 86 crates compiladas; 92 pacotes no `rust/Cargo.lock`.
- `.app` de 2,0 MB (binário de 1,9 MB, com o mesmo tamanho em bytes do instalado em
  `/Applications`). Depois do build: `rust/target` com 441 MB e `.build` com 127 MB.
- Exige o Xcode completo pelo `swift build` com SwiftUI.

### Comparação

- Build limpo: 4 min 11 s (protótipo) contra 1 min 51 s (controle), 2,3 vezes mais tempo.
- Crates: 397 contra 86. Tamanho do `.app`: 14 MB contra 2,0 MB, 7 vezes maior.

## Experimento: sem o poll e com o display link parado

Feito depois do resultado, a pedido do usuário, para separar o custo do app do custo do GPUI.
Branch `exp/gpui-parado` (commit 309d9d9, a partir de e197121), numa worktree; a `proto/gpui`
não foi tocada. Quatro variantes do mesmo protótipo:

- **base:** o código da `proto/gpui`, com o GPUI da tag.
- **sem-poll:** o poll de 10 Hz só roda tocando. Parado, o `Store` dorme e acorda por comando,
  carga ou sinal.
- **display-link:** o `gpui_macos` vendorizado com um patch de 149 linhas. O macOS passa a ter o
  `frame_waker` que o GPUI já usa em outras plataformas, e o `CVDisplayLink` para um quadro
  depois do último pedido de quadro. Na tag ele fica ligado sempre com a janela visível.
- **ambos:** as duas mudanças.

Mesma máquina e mesmo roteiro da medição 1, em 11/10, com janelas de 20 s e duas rodadas. As
rodadas foram intercaladas: a 1ª de todas as variantes, depois a 2ª. Sem macos-use. O controle
SwiftUI foi medido de novo só vazio, para ver a deriva da máquina. Tocando, o slider ficou em
9% em todas as rodadas, conferido por captura. Cada item traz a mediana, a média e o máximo de
CPU (%), uma rodada por linha.

### Vazio, na frente

- Controle: 0,40 / 2,37 / 9,00 e 0,30 / 0,30 / 1,00.
- base: 1,30 / 2,54 / 7,60 e 1,10 / 1,10 / 1,50.
- sem-poll: 1,05 / 1,05 / 1,40 e 0,80 / 0,81 / 1,30.
- display-link: 0,20 / 0,24 / 0,70 e 0,20 / 0,20 / 0,60.
- ambos: 0,00 / 0,02 / 0,40 e 0,00 / 0,58 / 4,10.

### Vazio, minimizado

- Controle: 0,20 / 0,23 / 0,30 e 0,30 / 0,59 / 3,90.
- base: 0,25 / 2,36 / 12,80 e 0,20 / 0,20 / 0,20.
- sem-poll: 0,00 / 0,01 / 0,20 nas duas.
- display-link: 0,20 / 0,23 / 0,40 e 0,20 / 0,24 / 0,50.
- ambos: 0,00 / 0,01 / 0,30 e 0,00 / 0,29 / 3,00.

O footprint vazio ficou em 32 a 36 MB em todas, controle incluído.

### Fila e faixa pausada, na frente

- base: 1,40 / 1,41 / 1,80 e 1,50 / 1,52 / 1,90.
- sem-poll: 0,90 / 1,02 / 1,70 e 1,10 / 1,15 / 1,50.
- display-link: 0,60 / 0,92 / 5,90 e 0,70 / 0,75 / 2,20.
- ambos: 0,40 / 0,41 / 0,80 e 0,40 / 0,39 / 0,70.

### Fila e faixa pausada, minimizado

- base: 0,60 / 0,62 / 0,80 e 0,70 / 0,66 / 1,00.
- sem-poll: 0,40 / 0,37 / 0,60 e 0,30 / 0,33 / 0,40.
- display-link: 0,70 / 0,67 / 0,90 e 0,50 / 0,49 / 0,70.
- ambos: 0,40 / 0,38 / 0,50 e 0,40 / 0,39 / 0,60.

Com a fila carregada, todas ficaram em 115 a 120 MB. Depois de rolar a fila inteira, a faixa foi
de 74 a 130 MB, sem padrão por variante: a sem-poll deu 130 MB numa rodada e 96 MB na outra.

### Tocando, na frente

- base: 5,55 / 5,29 / 6,30 e 5,60 / 5,46 / 6,00.
- sem-poll: 5,30 / 5,21 / 6,30 e 4,25 / 4,28 / 5,30.
- display-link: 4,20 / 4,34 / 5,80 e 5,35 / 6,55 / 14,90.
- ambos: 5,50 / 5,20 / 5,80 e 5,25 / 4,91 / 5,70.

Footprint tocando: 78 a 87 MB.

### Conferência visual do display-link

Feita à mão, no fone a 9%, com capturas de tela:
- a janela abre com a fila;
- o hover das linhas aparece em menos de 150 ms;
- o tooltip do selo aparece;
- o relógio avança tocando;
- o título muda na navegação;
- o cruzamento de 400 ms do fundo passa por cerca de 8 quadros capturados entre 0 e 550 ms,
  saindo de uma capa e chegando à outra.

Não foram conferidos o redimensionamento, o arrasto da janela e o ProMotion. No ProMotion, a
variante perde o `present` sustentado por 1 s depois de entrada rápida.

### Leitura rápida

- A base reproduz a medição 1: 1,40 e 1,50% com a fila na frente, contra 1,45 e 1,50% antes.
  Vazio, ela deu 1,10 e 1,30%, contra 0,70 e 0,80% antes. O controle vazio também subiu, de 0,20
  para 0,30 e 0,40%: a máquina estava um pouco mais ruidosa.
- O display link custa cerca de 0,8 a 1,0 ponto com a janela na frente:
  - vazio, de 1,10 e 1,30% para 0,20%;
  - com a fila, de 1,40 e 1,50% para 0,60 e 0,70%.
  Minimizado ele já está parado na tag, e as variantes empatam.
- O poll custa cerca de 0,2 a 0,3 ponto:
  - vazio na frente, de 0,20% para 0,00% com o link parado;
  - vazio minimizado, de 0,20 e 0,25% para 0,00%.
- Sem os dois, sobram 0,40% com a faixa pausada, na frente e minimizado. Vazio, o resto é 0. O
  resíduo vem de algo que fica ativo com o device tomado em pausa, fora do poll e do desenho.
  Não foi investigado.
- Tocando, as quatro empatam dentro do ruído (4,2 a 5,6%). O link ligando e desligando a cada
  tique não aparece no custo.
