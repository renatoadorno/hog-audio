# Protótipo de UI em GPUI: resultado

Data: 2026-10-11
Spec: `docs/superpowers/specs/2026-09-27-prototipo-gpui-design.md`
Plano: `docs/superpowers/plans/2026-09-27-prototipo-gpui.md`
Medições brutas: `docs/superpowers/specs/2026-09-27-prototipo-gpui-medicoes.md`
Estado: branch `proto/gpui`, com o experimento, mesclada na `main` local em 11/10/2026; sem push.

## Veredito

**O GPUI reprova no eliminatório de custo parado.** Com a fila carregada, uma faixa pausada e a
janela visível (a medição 1 da spec), o protótipo gasta 1,45 a 1,50% de CPU (mediana por
rodada) contra 0,6 a 0,7% do SwiftUI, cerca de 2 vezes. Com o app vazio, 0,7 a 0,8% contra 0,2%,
cerca de 4 vezes. A variação entre rodadas é de um décimo de ponto, muito menor que a diferença.

Os outros dois eliminatórios não reprovaram, com ressalvas:
- **Tocando:** empate, mediana de 6,05 a 6,15% no protótipo contra 6,25 a 6,55% no controle. O
  custo é do core, que é o mesmo.
- **Underrun sob estresse:** nenhum em 60 s tocando com rolagem e slider. É a versão curta da
  medição 6, sem redimensionar a janela e sem o Audio System Trace.
- **VoiceOver que não lê a fila:** a árvore de acessibilidade lê cada linha da fila (título,
  artista e álbum, selo, duração e estado), conferida pelo macos-use. O VoiceOver em si não foi
  ligado.

Pela regra combinada antes de medir, um eliminatório basta: o GPUI não substitui o SwiftUI.
Não há ADR a escrever.

**A favor do GPUI no que classifica:**
- **Visual:** na avaliação do usuário, ao usar os dois, o protótipo ficou "muito mais bonito e
  clean".
- **Abertura quente:** 216 ms contra 264 ms.
- **Memória depois de percorrer a fila:** 82 a 85 MB contra 89 a 93 MB.
- **Testes:** a UI tem 175 testes em Rust com o `TestAppContext`, inclusive cliques e teclas
  simulados.

**Contra:**
- Build 2,3 vezes mais lento e `.app` 7 vezes maior.
- Mais memória logo depois de carregar a fila (cerca de 40 MB a mais).
- A acessibilidade exige tirar o cache das views, e no macOS a ativação não desliga.
- A barra de título só arrasta nos 28 pt de cima.

Os números absolutos são pequenos (1,5% de um núcleo). O experimento feito depois separou as
causas. Cerca de 0,8 a 1,0 ponto vem do display link do `gpui_macos`, que na tag fica ligado
sempre que a janela está visível. Cerca de 0,2 a 0,3 ponto vem do poll de 10 Hz do app. O custo
parado é, na maior parte, do GPUI e não do protótipo. Os números estão na seção "Experimento".

## Experimento: de onde vem o custo parado

Feito depois do veredito, a pedido do usuário. Foi medido na branch `exp/gpui-parado` (309d9d9)
e depois mesclado na `proto/gpui` a pedido do usuário. Cada variante fica atrás de uma feature
do `hog-player-gpui`: `exp-sem-poll` e `exp-display-link`. Sem as features, o build é a base.
Os números brutos estão na seção "Experimento" das medições.

Quatro variantes do protótipo:
- **base:** o código desta branch;
- **sem-poll:** o poll de 10 Hz só roda tocando;
- **display-link:** o `gpui_macos` vendorizado com um patch de 149 linhas. O macOS ganha o
  `frame_waker` que o GPUI já usa em outras plataformas, e o `CVDisplayLink` para um quadro
  depois do último pedido de quadro;
- **ambos:** as duas mudanças juntas.

O que a tag faz, lido no código:
- o display link fica ligado o tempo todo com a janela visível e só para quando ela some
  (minimizada, oculta, coberta, em outro Space ou com o display dormindo);
- a cada vsync, o GPUI faz duas atualizações da janela mesmo sem nada a desenhar;
- o macOS não implementa o `frame_waker` (`gpui/src/platform.rs:938`).

Resultados, em mediana de CPU, com janelas de 20 s e duas rodadas intercaladas:

- **Vazio, na frente.** A base ficou em 1,10 e 1,30%. Sem o poll, 0,80 e 1,05%. Com o link
  parado, 0,20%. Com os dois, 0,00%. O controle, medido no mesmo dia, deu 0,30 e 0,40%.
- **Fila com faixa pausada, na frente.** A base ficou em 1,40 e 1,50%, reproduzindo a medição 1.
  Sem o poll, 0,90 e 1,10%. Com o link parado, 0,60 e 0,70%, o nível do controle na medição 1.
  Com os dois, 0,40%.
- **Minimizado.** O link já está parado na tag. Tirar o poll leva o app vazio de 0,20 e 0,25%
  a 0,00%. Com a faixa pausada, sobram 0,40% nas variantes sem poll. Esse resíduo, com o device
  tomado em pausa, não foi investigado. Na medição 1, a parte dele no controle não foi separada.
- **Tocando.** As quatro empatam dentro do ruído, de 4,2 a 5,6%. O link ligando e desligando a
  cada tique não aparece no custo.
- **Memória.** Não muda com as variantes.

A variante display-link foi conferida à mão com capturas:
- a fila aparece;
- o hover e o tooltip aparecem;
- o relógio avança tocando;
- o cruzamento de 400 ms do fundo anima até o fim.

O redimensionamento, o arrasto da janela e o ProMotion não foram conferidos.

**O que isso faz com o veredito.** A regra deste doc era refazer a comparação se o custo parado
caísse ao nível do SwiftUI, e tratá-lo como custo do GPUI, que vale também para o vitral, se não
caísse. As duas metades valem:
- **Do GPUI:** o custo é do GPUI da tag, não do app.
- **Removível:** só remendando o `gpui_macos`. A v1.21 não tem API para parar o link.
- **No nível do SwiftUI:** com o patch, o protótipo chega ao nível do SwiftUI, e fica abaixo
  dele com o poll também desligado.

O veredito acima continua valendo para o GPUI como ele é publicado. Refazer a comparação com o
patch é decisão do usuário, e o patch traz custos:
- um `gpui_macos` próprio para manter a cada tag. É o que o vitral seria;
- sem o poll, a suspeita de device perdido, o problema novo do engine e o volume mudado por fora
  com o player pausado esperam o próximo comando;
- no ProMotion, perde-se o `present` sustentado por 1 s depois de entrada rápida;
- o `[patch]` do Cargo vale para o workspace inteiro, então o `gpui_macos` sempre vem de
  `apps/gpui/vendor/`, mesmo sem a feature. Sem ela, o código vendorizado é o da tag. Trocar de
  tag exige trocar a cópia junto.

## O que foi construído

- Workspace Cargo em `apps/gpui`, separado do `rust/`, com `Cargo.lock` e `target/` próprios e
  o GPUI do Zed v1.21.0 por git. Três crates:
  - `hog-player-model` (2.031 linhas): controlador portado do `PlayerViewModel.swift`, formatos,
    estados de tela, leitura de tags com `lofty`, miniatura, capa e fundo desfocado. Não depende
    de GPUI.
  - `hog-player-gpui` (2.401 linhas): janela, store, views com sinais por parte do modelo,
    menus, saída que devolve o device, acessibilidade.
  - `hog-player-testing` (541 linhas): o player falso, com portões para provar ordem e
    concorrência.
- Testes: 3.880 linhas em `tests/`; 175 testes passam no gate (`make gpui-test`), mais 4
  processos filhos ignorados que os testes de sinal disparam.
- Medidores externos em `apps/gpui/bench/` (Swift): `window_timer` e `input_probe`, que medem os
  dois apps sem tocar em nenhum deles.
- Do Pawse (MIT): a técnica do fundo desfocado (capa reduzida, `fast_blur` e saturação, em
  BGRA), o slider adaptado (sem o drag do GPUI, que anulava o cache da janela inteira) e o
  padrão de menus com `actions!`.

## Bit-perfect

- `git diff main -- rust/` vazio do começo ao fim; o gate `check_core_untouched.sh` compara a
  árvore do core (nome, versão e features) com o `rust/Cargo.lock`, com 16 exceções justificadas.
- O protótipo usa só a API pública de `hog_audio::api` e `PlayerState`.
- `make verify*` refeito com as bindings do SDK 27.0: 24/96 e 16/44.1 bit-perfect, pausa e
  volume idênticos.
- Faixa acima do que o device aceita é recusada com a mensagem do core ("Reproduzir exigiria
  resample"), sem resample.

## As medições

Com a biblioteca de 221 faixas, janelas de 20 s e duas rodadas por app (escopo reduzido pelo
usuário: CPU e RAM, sem energia e sem `sudo`).

1. **Parado:** vazio na frente, protótipo 0,70 e 0,80% contra 0,20 e 0,20%; com fila e pausado,
   1,45 e 1,50% contra 0,60 e 0,70%. Minimizado: vazio 0,25 e 0,20% contra 0,20 e 0,20%; com
   fila 0,60 e 1,35% contra 0,70 e 0,70%.
2. **Tocando:** 6,15 e 6,05% contra 6,25 e 6,55%; 98 e 86 MB contra 81 e 80 MB.
3. **Memória:** vazio 32 a 33 MB contra 31 a 32 MB. Com a fila carregada, 117 e 122 MB contra 78
   e 82 MB. Depois de rolar a fila inteira, 85 e 82 MB contra 89 e 93 MB.
4. **Abertura quente:** mediana de 216 ms contra 264 ms (cinco rodadas cada). A fria ficou de
   fora (exige `purge`).
5. **Resposta (parcial):** trocas de faixa de 41 a 135 ms, quase todo o tempo no core; cliques
   rápidos em Próxima se juntam (navegação 1 e depois 2); Delete fora da faixa atual em 0 ms, sem
   corte. Slider, clique e hitches não foram medidos nos dois apps.
6. **Estresse (curta):** sem underrun em 60 s.
7. **VoiceOver:** ver abaixo.
8. **Integração:** ver abaixo.
9. **Pânico:** o processo aborta (pânico num callback `extern "C"` do AppKit), o hook escreve
   "pânico: device devolvido" e o device volta ao rate, volume e dono de antes.
10. **Build:** limpo em 4 min 11 s contra 1 min 51 s; 397 crates contra 86; `.app` de 14 MB
    contra 2,0 MB. Os dois exigem o Xcode completo.

## Medição 7: acessibilidade (sim ou não, protótipo)

- Cada linha da fila lê título, artista e álbum, selo, duração e estado: sim, pela árvore.
- Botões com nome (Tocar/Pausar, Anterior, Próxima, Abrir, Limpar): sim.
- Slider ajustável: sim por Increment/Decrement; `SetValue` mandado como texto é ignorado.
- Delete e ⌘O sem mouse: sim.
- Aviso alcançável: o banner tem papel de alerta no código; não foi conferido na travessia.
- **Custo achado no uso real:** no GPUI v1.21, as views em cache somem da árvore depois de
  quadros parciais. A correção tira o cache das views quando a acessibilidade está ativa. No
  macOS a ativação não desliga até o app fechar, e qualquer cliente de acessibilidade a liga
  (VoiceOver, macos-use, utilitários). Quem usa leitor de tela paga o app sem cache.
- O lado do SwiftUI não foi medido com o VoiceOver.

## Medição 8: integração (sim ou não, protótipo)

- ⌘O no menu e como atalho: sim.
- Filtro de tipos no painel de abrir: não (o `prompt_for_paths` do GPUI não filtra).
- Arrastar do Finder: sim por teste; não exercitado à mão.
- Barra transparente: sim; arrastar a janela só funciona nos 28 pt de cima. A faixa do botão
  Abrir não arrasta.
- Tooltip no selo: no código; não exercitado à mão (o macos-use não passa o mouse por cima).
- Delete e espaço: sim.
- ⌘Q, Ctrl+C, SIGTERM, menu Fechar e botão vermelho devolvem o device com hog −1, o rate e o
  volume de antes: sim, tocando, no fone, nas caixas e no monitor por HDMI.
- Desplugar o fone tocando: para, avisa e não toca sozinho; o Tocar seguinte vai às caixas no
  teto de 50%, com aviso.

## Diferenças deliberadas

- **Lista de extensões no lugar do `UTType.audio`:** o Rust não tem o UTType. A lista recusa
  m4b, m4r, au, snd, amr, ac3, ogg, opus, wma, ape e wv, que o `UTType.audio` aceita.
- **Delete na janela inteira:** o protótipo não tem campo de texto; no SwiftUI ele vale só com a
  lista em foco.
- **Timer sem folga:** o timer do GPUI não expõe tolerância; o do SwiftUI tem 20 ms. A spec
  previa trocá-lo por um timer de GCD com folga se o poll pesasse. O experimento foi além e
  parou o poll fora da reprodução. O ganho foi de 0,2 a 0,3 ponto, menos que o do display link.
- **`lofty` no lugar do AVFoundation:** lê só as tags, sem as propriedades do áudio; o selo
  continua vindo do `probe_track` do core.
- **Painel de abrir sem filtro:** limitação da API do GPUI.
- **⌘O também no menu:** no SwiftUI ele é só atalho do botão da barra.

## O que ficou de fora

- Energia, potência de GPU e abertura a frio (sem `sudo`, decisão do usuário).
- Janela de 1280×800 nos dois (o ajuste pelo macos-use ligaria a acessibilidade); o controle
  mediu em 980×692 e o protótipo em 920×740.
- A biblioteca de 1.157 faixas (medido com 221).
- Slider, clique e hitches da medição 5; o estresse de 5 minutos com Audio System Trace; o
  VoiceOver ligado; a medição 8 do lado do controle.
- O laço de escritas de volume recusadas (lido no código) não foi reproduzido: o único device
  sem controle de volume à mão não recebe escrita nenhuma.

## Riscos que ficam

- **`objc2` #861:** o lock tem o `objc2` 0.6.3 e o 0.5.2, provavelmente afetados pelo
  use-after-free em release do #861. Em 27/09 a correção estava só no main do `objc2`, com
  backport anunciado para 0.6.5 e 0.5.3 e sem release. A versão não foi trocada, por decisão do
  usuário no passo 0.
- **Pânico no IOProc:** o `io_proc` do core (`rust/src/playback.rs`) é `extern "C"` e roda na
  thread de IO sem `catch_unwind`. Um pânico ali aborta o processo, e o hook do protótipo só
  devolve o device em pânico da main thread. Nesse caso o rate e o volume não voltam. É
  improvável, e vale igual para o controle, que usa o mesmo core.
- **A raiz renderiza a cada quadro desenhado:** o GPUI refaz o layout da view raiz em todo
  quadro (`gpui/src/window.rs:3418` na tag). Só as filhas em cache são poupadas, e o cache sai
  com a acessibilidade ativa.

**Passo seguinte, se valer insistir no GPUI:** refazer a comparação com a variante "ambos" do
experimento e decidir se o patch do `gpui_macos` entra no vitral ou se vale propor ao Zed o
`frame_waker` no macOS. O patch precisa antes ser conferido em redimensionamento, em arrasto e
no ProMotion.
