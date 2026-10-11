# Protótipo de UI em GPUI: resultado

Data: 2026-10-11
Spec: `docs/superpowers/specs/2026-09-27-prototipo-gpui-design.md`
Plano: `docs/superpowers/plans/2026-09-27-prototipo-gpui.md`
Medições brutas: `docs/superpowers/specs/2026-09-27-prototipo-gpui-medicoes.md`
Estado: na branch `proto/gpui`, commitado; sem push.

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

Os números absolutos são pequenos (1,5% de um núcleo). A causa provável é a do enunciado da
spec: o display link do GPUI acorda a main thread com a janela visível (a thread `CVDisplayLink`
aparece viva no `sample`), somado ao poll de 10 Hz sem folga. Minimizado, a diferença quase some
com o app vazio (0,20 a 0,25% contra 0,20%).

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
- **Timer sem folga:** o timer do GPUI não expõe tolerância; o do SwiftUI tem 20 ms. Pode pesar
  no custo parado.
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

**Passo seguinte, se valer insistir no GPUI:** separar o custo do app do custo do GPUI, medindo
o protótipo sem o poll de 10 Hz (ou com o poll acordado só por mudança) e com o display link
parado quando nada anima. Se o parado cair ao nível do SwiftUI, a comparação se refaz; se não,
o custo é do GPUI e vale para o vitral também.
