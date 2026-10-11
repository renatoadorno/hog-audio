# Medidores externos

Dois instrumentos que medem o app SwiftUI de controle e o protótipo GPUI do mesmo jeito, por fora, sem tocar em nenhum dos dois e sem usar a API de acessibilidade.

- `window_timer <executável> [rodadas]`: tempo do lançamento até a primeira janela do processo (camada 0) aparecer na tela, lido pelo `CGWindowList`, com amostragem a cada 5 ms. Termina o app com SIGTERM, que os dois tratam devolvendo o device; se ele não sair em 5 s, manda SIGKILL e anota na linha da rodada. Espera 2 s entre rodadas. Mede a janela no WindowServer, não o primeiro quadro desenhado: por isso a medição 4 confere com o `xctrace`.
- `input_probe`: gera o gesto com `CGEvent`, lê a janela pelo `CGWindowList` e o device por Core Audio, só leitura. O device vem sempre pelo UID, porque sob hog o "default output" aponta para outro.
  - `list`: id, UID e nome de cada device.
  - `read <uid>`: volume, se está rodando, sample rate nominal e o dono do hog (`-1` é livre).
  - `frame <pid>`: id, x, y, w e h de cada janela de camada 0 do processo na tela, da frente para trás.
  - `slider <x1> <y1> <x2> <y2> <uid>`: lê o volume, arrasta em 30 passos de 10 ms e mede do soltar até a última mudança de volume no hardware, numa janela de 2 s. Imprime o volume de antes e o de depois.
  - `click <x> <y> <uid>`: clica e mede até o device parar e voltar a rodar, com limite de 3 s.
  - `scroll <x> <y> <linhas> <vezes>`: rola por linhas, um evento a cada 16 ms.
  - `drag <x1> <y1> <x2> <y2>`: pousa o cursor, aperta e arrasta em 30 passos; serve para mover a janela pela barra de cima e redimensioná-la pelo canto.
  - `key <keycode> [cmd] [shift] [alt] [ctrl]`: aperta e solta uma tecla, com os modificadores pedidos. Keycodes usados aqui: Delete (⌫) 51, espaço 49, seta para baixo 125, seta para cima 126, Q 12 (com `cmd`, ⌘Q).

Códigos de saída do `window_timer`: 0 ok, 1 a janela não apareceu em 15 s, 2 uso, 3 o executável não lançou, 4 o app terminou antes de abrir a janela. Do `input_probe`: 0 ok, 1 device ou janela não encontrados (ou o `click` não viu o device parar e voltar), 2 uso, 3 sem permissão para postar eventos.

## Compilar

Da raiz do repositório:

```bash
mkdir -p apps/gpui/bench/build
swiftc -O -o apps/gpui/bench/build/window_timer apps/gpui/bench/window_timer.swift
swiftc -O -o apps/gpui/bench/build/input_probe apps/gpui/bench/input_probe.swift
```

`apps/gpui/bench/build/` fica fora do git.

Conferência contra casos conhecidos, sem device:
- `window_timer /System/Applications/TextEdit.app/Contents/MacOS/TextEdit 2` imprime dois tempos na casa das centenas de ms e fecha o TextEdit.
- `input_probe list` mostra o fone (no M1, `BuiltInHeadphoneOutputDevice`).
- `input_probe read <uid do fone>` mostra o volume atual, `rodando false`, o rate e `hog -1`.

## Permissões e foco

- `slider`, `click`, `scroll`, `drag` e `key` postam eventos: o app de terminal que roda o comando precisa estar em Ajustes do Sistema > Privacidade e Segurança > Acessibilidade. Sem ela, o `input_probe` para antes do gesto com saída 3, em vez de o macOS descartar os eventos em silêncio.
- Os eventos de mouse movem o cursor de verdade e vão para o que estiver sob o ponto; os de teclado vão para a janela-chave do app ativo. Antes de cada gesto, trazer o app para a frente sem acessibilidade e não mexer no mouse nem no teclado durante o gesto:
  - controle: `open -a /Applications/HogAudio.app` ou `osascript -e 'tell application id "local.hogaudio.player" to activate'`;
  - protótipo: `open -a apps/gpui/build/HogAudioGPUI.app` ou `osascript -e 'tell application id "local.hogaudio.gpui" to activate'`.
  - O `osascript` pode pedir a permissão de Automação na primeira vez.
- As coordenadas são pontos globais com origem no canto superior esquerdo da tela principal: as mesmas do `frame` e da árvore do macos-use.
- `screencapture` de outro app pede a permissão de Gravação de Tela para o terminal.
- `list`, `read`, `frame` e o `window_timer` não postam eventos e não precisam da permissão de Acessibilidade.

## Acessibilidade deixa o GPUI sem cache

Qualquer cliente de acessibilidade liga a acessibilidade do protótipo, e daí em diante as views rodam sem cache até a janela fechar (`apps/gpui/app/src/views/root.rs`). No macOS a ativação é pegajosa: nada a desliga. Medir depois disso mede a versão lenta.

- Ligam a acessibilidade: o macos-use, o VoiceOver, o Accessibility Inspector e o UI scripting (System Events). Utilitários em segundo plano com permissão de Acessibilidade (gerenciadores de janela, lançadores, expansores de texto) podem ligar também: fechar durante as medições.
- Só um app fica aberto por vez. Depois de usar qualquer cliente de acessibilidade, fechar o app com ⌘Q e relançá-lo antes das medições 1, 2, 3, 5 e 6; o outro já está fechado. Cada app medido sai de um lançamento limpo, o que mantém a comparação justa.
- A medição 7 (VoiceOver) fica por último, ou o app é relançado depois dela.
- Nenhum log confirma o estado: o protótipo não inicializa logger, então o "Accessibility activated" do GPUI não sai em lugar nenhum. A garantia é relançar sem nenhum cliente de acessibilidade aberto.
- `open -a`, `osascript ... activate`, `frame`, `drag`, `key`, `screencapture` e o `window_timer` não usam acessibilidade.

## Aviso: medições que tomam o device

As medições 1, 2, 5, 6, 8 (espaço, ⌘Q e Ctrl+C) e 9 tomam o device em hog: enquanto rodam, nenhum outro app toca nele. Cada uma só roda depois de avisar o usuário e ter o ok dele, uma de cada vez. `powermetrics` e `purge` pedem `sudo`: o usuário roda com `! <comando>` ou autoriza.

## Aviso: volume no fone

O slider escreve o volume do hardware. Arrastado até o fim do trilho com música tocando, ele leva o volume do fone a 1,0, e as medições 5 e 6 repetem o gesto dezenas de vezes.
- Começar cada sessão com o volume do app baixo.
- Arrastar só entre 20% e 40% do trilho (a conta está no passo 6 da Montagem), nunca até o fim.
- Alternar o sentido a cada gesto: de `x1` para `x2`, depois de `x2` para `x1`.

## Montagem

Conferida antes da primeira medição:
- MacBook Air M1 na bateria, brilho fixo (anotar o valor), só o Terminal aberto e um app aberto por vez.
- Controle: `/Applications/HogAudio.app/Contents/MacOS/HogAudio` (main `a11fe30`, bundle id `local.hogaudio.player`). Protótipo: `apps/gpui/build/HogAudioGPUI.app/Contents/MacOS/HogAudioGPUI` depois do `make gpui-app` (bundle id `local.hogaudio.gpui`).
- O pid sai de `pgrep -x HogAudio` ou `pgrep -x HogAudioGPUI`.
- A biblioteca do usuário (1.157 faixas com capa) baixada localmente: perguntar o caminho e conferir com `find <pasta> -name '*.flac' | head` e `ls -lO` (sem `dataless`).
- O UID do device vem do `input_probe list`.

A tela precisa de espaço para 1280×800. No Air em 1440×900 a janela só cabe depois de ir para perto da origem: o protótipo abre centrado em 920×740, com a origem perto de (260, 80).

Janela e coordenadas, nesta ordem, um app por vez:
1. Lançar o app e ler a janela com `input_probe frame <pid>`.
2. Mover a janela para perto da origem, pela barra de cima: `input_probe drag <x+w/2> <y+10> <w/2> <35>`. O ponto fica a 10 pt do topo e no meio da largura, longe dos botões de semáforo e do botão Abrir; no protótipo só os ~28 pt de cima arrastam a janela. Conferir com `frame` (o macOS pode segurar o y logo abaixo da barra de menus).
3. Com o `x y w h` novo, ajustar o tamanho pelo canto inferior direito, sem macos-use: `input_probe drag <x+w-1> <y+h-1> <x+1279> <y+799>`. Conferir com `frame` até sair `w 1280  h 800`. Se o tamanho não mudou, o ponto errou o canto: repetir 2 pt para fora, de `x+w+2 y+h+2` até `x+1282 y+802`. Se a altura parar antes de 800, esconder o Dock (⌥⌘D) e repetir.
4. Ler pelo macos-use, uma vez só, os campos `x y w h` do trilho do slider, de duas linhas vizinhas da fila (a vizinha sai de `y ± h` da linha lida), de um ponto da fila para rolar e do botão Próxima. Guardar cada um como deslocamento da origem da janela (x do elemento menos x da janela; o mesmo para y). Para a medição 6, repetir num segundo tamanho menor (por exemplo 1100×700): o tamanho da capa depende do tamanho da janela, e o slider e a fila mudam de lugar com ela. Este passo liga a acessibilidade.
5. Fechar o app com ⌘Q. Para medir, relançá-lo, mover e redimensionar como nos passos 2 e 3 e seguir com os deslocamentos guardados, somados à origem que o `frame` mostrar.
6. Trilho do slider a partir de `x y w h`: `y = y + h/2`, `x1 = x + 0,2·w` e `x2 = x + 0,4·w`. Nunca usar o fim do trilho (ver o aviso de volume).

Os números brutos vão para `docs/superpowers/specs/2026-09-27-prototipo-gpui-medicoes.md`: uma seção por medição, três rodadas e a mediana, sem tabelas.

## Protocolo

Medição 1, custo parado (toma o device):
- Biblioteca na fila (o caminho como argumento do executável), uma faixa tocada e pausada, janela na frente, 60 s sem mexer.
- `sudo powermetrics --samplers tasks --show-process-energy -i 1000 -n 60 > m1-<app>-<rodada>.txt`
- Por amostra, na linha do processo: CPU ms/s, "interrupt wakeups", "idle wakeups" e "energy impact". Registrar a mediana das 60 por rodada.
- Repetir com a janela minimizada. Três rodadas por app, alternando a ordem.

Medição 2, tocando (toma o device):
- Uma faixa 24/96 tocando por 60 s, janela na frente.
- `sudo powermetrics --samplers tasks,gpu_power --show-process-energy -i 1000 -n 60`
- As métricas da medição 1 mais a potência da GPU.

Medição 3, memória:
- `footprint <pid> | grep -i footprint` logo depois de abrir, com a janela vazia.
- De novo com a biblioteca na fila e as linhas preenchidas, depois de a CPU do processo voltar ao repouso.
- De novo depois de rolar a fila inteira com `input_probe scroll <x> <y> -5 <vezes>` sobre ela (a Task 13 sugere 400). Conferir o fim da fila com `screencapture -l <id da janela, do frame> m3-<app>.png` (não pelo macos-use) e aumentar `vezes` se não chegou.

Medição 4, abertura:
- Frio, cinco rodadas por app, com `sudo purge` antes de cada uma: `apps/gpui/bench/build/window_timer <executável> 1`.
- Quente, cinco rodadas seguidas: `apps/gpui/bench/build/window_timer <executável> 5`.
- Conferência, uma rodada por app: `xcrun xctrace record --template 'App Launch' --launch -- <executável>`, anotando o tempo até o primeiro quadro que o Instruments mostra.

Medição 5, resposta (toma o device). Começa de um lançamento limpo, com a janela movida e redimensionada como na Montagem e o app trazido para a frente antes de cada gesto. Nenhum passo usa o macos-use; se precisar dele, relançar antes de continuar.
1. Slider, dez vezes por app, tocando, alternando o sentido: `input_probe slider <x1> <y> <x2> <y> <uid>` e depois `input_probe slider <x2> <y> <x1> <y> <uid>`.
2. Hitches do slider: `xcrun xctrace record --template 'Animation Hitches' --attach <pid> --time-limit 10s --output m5-slider-<app>.trace` com o `input_probe slider` em paralelo; exportar com `xcrun xctrace export --input <trace> --toc` e contar os hitches. Se o template não enxergar os quadros do GPUI (Metal próprio), usar `--template 'Metal System Trace'` e medir o p99 do intervalo entre quadros.
3. Rolagem: `input_probe scroll <x> <y> -5 <vezes>` com o `Animation Hitches` gravando, conferindo o fim da fila por `screencapture`.
4. Clique na fila, dez vezes por app, com uma faixa tocando: duas linhas vizinhas de formato diferente (o formato de cada uma se confere pelo selo, num `screencapture`), clicando uma de cada vez com `input_probe click <x> <y> <uid>`. O device tem de parar e voltar.
5. Delete, dez vezes por app: clicar numa linha do meio com `input_probe click <x> <y> <uid>` (nos dois apps o clique também toca a linha e, no SwiftUI, dá foco à lista), `input_probe key 125` para selecionar a de baixo sem tocá-la e `input_probe key 51` para apagá-la. Conferir a linha removida com um `screencapture` antes e outro depois.
6. Encher a fila: lançar o app com o caminho da biblioteca e medir com `top -l 0 -s 1 -pid <pid> -stats cpu` o tempo até a CPU ficar abaixo de 2% por 3 s seguidos. Registrar à parte: os leitores de tag são outros (AVFoundation e lofty).
7. Fallback: se o `input_probe read` mostrar que o HAL recusa ler o device em hog de outro processo, usar as linhas de transição: `log show --predicate 'subsystem == "local.hogaudio.player"' --last 5m` no SwiftUI e `~/Library/Logs/hog-player-gpui.log` no protótipo.

Medição 6, áudio sob estresse (toma o device):
- Cinco minutos tocando uma faixa 24/96 (repetir a faixa se precisar).
- Os gestos rodam em série, num laço só, nunca dois ao mesmo tempo. A cada volta de uns 5 s, uma rajada de `input_probe scroll` (alternando o sentido). A cada 10 s, um `input_probe drag` no canto inferior direito, alternando entre 1280×800 e o tamanho menor da Montagem (nunca pelo macos-use). A cada 15 s, um `input_probe slider` entre 20% e 40% do trilho, alternando o sentido. A cada minuto, Próxima com `input_probe click` no botão.
- As coordenadas de cada gesto são as do tamanho atual da janela, pré-medidas nos dois tamanhos (passo 4 da Montagem).
- Em paralelo, por não ser gesto: `xcrun xctrace record --template 'Audio System Trace' --attach <pid> --time-limit 300s`.
- Passa se a linha técnica termina sem "underruns" e o trace não mostra `ProcessorOverload`.

Medição 7, VoiceOver, por último (liga a acessibilidade), com o VoiceOver ligado (⌘F5) e o Accessibility Inspector aberto; sim ou não por item e por app:
- Cada linha da fila lê título, artista, duração e selo.
- Transporte, Abrir e Limpar têm nome.
- O slider ajusta com VO + setas.
- Delete e ⌘O funcionam sem mouse; registrar a diferença do Delete (a janela inteira no GPUI, a lista em foco no SwiftUI).
- O aviso de erro, provocado soltando um arquivo que não é áudio, é alcançável.

Medição 8, integração com o Mac; sim ou não por app (espaço, ⌘Q e Ctrl+C tomam o device):
- ⌘O no menu e como atalho.
- Filtro de tipos no painel de abrir (o GPUI não filtra: registrar).
- Arrastar três arquivos e uma pasta de uma vez do Finder.
- Barra de título transparente, tamanho mínimo e tooltip no selo.
- Delete e espaço (Tocar/Pausar).
- ⌘Q devolve o device: tocando, ⌘Q, e `input_probe read <uid>` mostra `hog -1` e o rate de antes.
- Ctrl+C devolve o device: lançar o executável no terminal, em primeiro plano, tocar, apertar Ctrl+C nesse terminal, e `input_probe read <uid>` mostra `hog -1` e o rate de antes.

Medição 9, pânico, só no protótipo (toma o device):
1. Da raiz: `(cd apps/gpui && cargo build --release -p hog-player-gpui --features panic-probe)`. O `make gpui-app` recompila sem a feature: não usar o `.app` aqui.
2. Rodar pelo terminal, para ver o stderr: `apps/gpui/target/release/hog-player-gpui testdata/t96_24.flac`.
3. Tocar a faixa e anotar o `input_probe read <uid>`.
4. Menu Depurar > Provocar pânico.
5. Registrar se o processo abortou, o que o stderr mostrou ("pânico: device devolvido" ou o teto) e o `input_probe read <uid>` logo depois (hog e rate).
6. Reconstruir sem a feature ao terminar (`make gpui-app`).

Medição 10, build:
- Protótipo: `cd apps/gpui && cargo clean && time cargo build --release -p hog-player-gpui`, `du -sh apps/gpui/build/HogAudioGPUI.app` depois do `make gpui-app` e o número de crates da Task 0.
- Controle: `rm -rf apps/player/.build && (cd rust && cargo clean) && time make app` e `du -sh apps/player/HogAudio.app`. Limpa o build do core (não o código) e o `make test` seguinte reconstrói: avisar o usuário antes.
- Anotar se cada um exige o Xcode completo (o GPUI pelo Metal Toolchain, o SwiftUI pelo `swift build`).

## Leitura dos resultados

- `slider` com `0.0 ms` e o volume de depois igual ao de antes: nada mudou, o gesto não pegou o slider. Com `0.0 ms` e o volume diferente: o valor já estava no fim ao soltar, aplicado durante o arrasto.
- `click` que não volta aponta para evento que não chegou (permissão, janela inativa ou coordenada fora da linha). Não é o formato: o core para e solta o device em toda troca de faixa, qualquer que seja o formato (`stop_and_release` no começo de cada `play`, em `rust/src/engine.rs`).
- `key` sem efeito: a tecla foi para outro app ou, no SwiftUI, a lista não estava em foco.
- Rodada do `window_timer` com "levou SIGKILL": o app não saiu sozinho; conferir o device com `input_probe read <uid>` antes da rodada seguinte.
