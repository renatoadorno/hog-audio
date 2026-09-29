# Protótipo de UI em GPUI: design

Data: 2026-09-27
Estado: aprovado em conversa, seção por seção; falta a revisão desta spec escrita.

## Objetivo

Construir um protótipo de UI em GPUI e medi-lo contra o app SwiftUI atual (`apps/player`).
O protótipo existe para a comparação e não substitui o app SwiftUI. Adotar o GPUI, se ele
vencer, é outra decisão, com ADR própria.

## Restrições

- **O core não muda.** `git diff main -- rust/` fica vazio do começo ao fim. Continua valendo:
  nada toca IOProc, ring, decodificador, negociação nem a ordem hog → rate → formato → volume,
  e `make verify*` roda antes de qualquer instalação.
- **O protótipo usa só a API pública do `HogPlayer`**, a mesma que o Swift usa, como biblioteca
  Rust direta, sem uniffi. O core já compila como `lib` e `staticlib`, e `HogPlayer::new()`
  devolve `Arc<Self>`.
- **O core não depende de nada de UI.** O `audio_engine` do Pawse depende do gpui, e isso não
  entra aqui.
- **Tecnologia web está fora.**
- **Commit e instalação só quando o usuário pedir.** Tudo que toma o device é avisado antes.

## Decisões

- **O GPUI vem do Zed v1.21.0 por git** (tag estável de 23/09/2026), crates `gpui` e
  `gpui_platform`. Isso mede o mesmo código de que o vitral vai derivar e prova se o GPUI compila
  fora do workspace do Zed.
  - O workspace do Zed tem um `[patch.crates-io]` (`Cargo.toml:996-1021`, com `async-task`,
    entre outros) que não vale fora dele. Resolver do zero muda 245 versões em relação ao lock
    do Zed.
  - O protótipo semeia o lockfile com o `Cargo.lock` do Zed na tag e replica os patches que
    entram na árvore do macOS. Detalhes em `docs/research/2026-09-27-gpui-v1.21-api.md`.
  - O `gpui_platform` precisa da feature `font-kit`; sem ela, a janela abre sem texto.
  - O `gpui-component` fica de fora, porque ele fixa `gpui-pre` na versão exata.
- **O protótipo mora em `apps/gpui/`**, como workspace Cargo à parte, com `Cargo.lock` e
  `target/` próprios e `hog-audio = { path = "../../rust" }`. São três crates:
  - `hog-player-model`, o controlador, sem GPUI (garantido pelo `Cargo.toml` dele);
  - `hog-player-gpui`, a UI;
  - `hog-player-testing`, o player falso, só para teste. Não é workspace
  com o `rust/`, e o lockfile do core não muda. A branch é a `proto/gpui`, criada a partir da
  `main`.
- **A orquestração fica num controlador próprio no crate, sem GPUI.** Ele porta as regras do
  `PlayerViewModel.swift` e é testado com `cargo test` e um player falso. As alternativas
  descartadas foram:
  - orquestrar numa `Entity` do GPUI, que prende a lógica à UI e os testes ao `TestAppContext`;
  - levar a orquestração para o core, que quebra a regra desta rodada.
- **Metadados via `lofty`** dentro do crate: título, artista, álbum e capa. Hoje eles vêm do
  AVFoundation, em `HogPlayerKit/Metadata.swift`. O `lofty` só lê tags; decodificar continua
  com o `ExtAudioFile` do core. O selo continua vindo do `probe_track` do core.

## O que se aproveita do Pawse

O Pawse está em `~/work/audio-toolbox/pawse`, sob licença MIT, e o aviso de copyright acompanha o
código copiado.

- A imagem do fundo desfocado (`crates/pawse/src/cover_backdrop.rs:250-270`): a capa é reduzida
  a 96 px, passa por `fast_blur`, ganha saturação e vai na ordem BGRA que o GPUI envia. Os
  testes vêm junto.
- O slider (`crates/ui_components/src/slider.rs`). Do `gpui-component` ele usa só as cores do
  tema.
- A miniatura com placeholder (`crates/ui_components/src/cover_thumb.rs`).
- O menu com `actions!` e `set_menus` (`crates/pawse/src/app_menu.rs`), e os atalhos por
  contexto (`keyboard_shortcuts.rs`).

**Não se aproveita:**
- A lista da fila. Ela usa o `v_virtual_list` do `gpui-component` com id por posição
  (`queue_view.rs:338`), o mesmo defeito que saiu do SwiftUI.
- A barra de título do `gpui-component`.
- O barramento `flume` e o resto do app: biblioteca, temas e preferências.

O Pawse não tem acessibilidade, arrastar do Finder nem painel de abrir. Isso sai direto da API
do GPUI: `on_drop::<ExternalPaths>` e `prompt_for_paths`.

## Paridade com o app SwiftUI

- **Janela.** Duas colunas: capa quadrada à esquerda e fila à direita. A conta de tamanho é a de
  `PlayerLayout.swift`: capa de 320 a 560 pt, fila com pelo menos 340 pt, janela com pelo menos
  820×640 pt e tamanho padrão 920×700 pt.
- **Fundo.** A capa desfocada, com cruzamento de uma capa para a outra na troca de faixa.
- **Faixa atual:**
  - título, artista e álbum;
  - relógio;
  - transporte: anterior, tocar ou pausar, próxima;
  - volume com o ícone de mudo;
  - linha técnica, com formato e underruns.
- **Fila:**
  - cada linha tem miniatura, título, "Artista — Álbum", selo `24/96` com tooltip de detalhe e
    duração;
  - uma linha que falhou mostra o motivo no tooltip;
  - clique toca a faixa, Delete remove a linha selecionada e o botão Limpar esvazia a fila;
  - a lista rola até a faixa atual.
  - **Diferença deliberada:** o Delete vale na janela inteira, porque não há campo de texto. No
    SwiftUI ele só funciona com a lista em foco, um defeito conhecido. A medição 7 registra a
    diferença.
- **Avisos.** O de erro e o de perda do device, com as mesmas mensagens do core.
- **Integração:**
  - ⌘O;
  - espaço para tocar e pausar;
  - arrastar do Finder, com destaque de alvo;
  - painel de abrir.

**Fora do escopo:** o que o SwiftUI também não tem, como seek, reordenar a fila, biblioteca e
preferências.

## Peças de `apps/gpui/`

- **`port.rs`.** A trait `Player` com os métodos do `HogPlayer` que o app usa, implementada
  para `hog_audio::api::HogPlayer`. O `FakePlayer` dos testes implementa a mesma trait.
- **`controller/`.**
  - O `Controller` cuida de comandos em série, Next somados, worker de volume, auto-avanço,
    disparo da recuperação, sequência do erro e shutdown.
  - Os estados puros são portes de `DisplayState.swift`, `Quality.swift`, `TimeFormat.swift` e
    `PlayerLayout.swift`.
  - O modelo se divide como no `@Observable`: `display`, `clock`, `queue` com `entries`,
    `volume` e `error`.
- **`media/`.**
  - `tags.rs` lê as tags com o `lofty`.
  - `artwork.rs` gera a miniatura e a imagem do fundo desfocado.
  - `audio_files.rs` é o porte de `AudioFiles.swift`.
  - Tudo devolve bytes e buffers RGBA, sem tipos do GPUI.
- **`ui/`.**
  - As views: raiz em duas colunas, `now_playing`, `clock`, `volume`, `queue`, `backdrop`,
    `status` e `theme` (tokens de cor e espaço).
  - A `queue` usa o `uniform_list`, com o id da faixa como id do elemento.
  - Cada parte do modelo é uma `Entity` separada: um tique do relógio só notifica a view do
    relógio.
  - A conversão para `gpui::Image` e `RenderImage` mora aqui.
- **`main.rs`.** Aplicação, janela com barra de título transparente e tamanho mínimo, menus e
  ações, `on_app_quit` e sinais.
- **`bench/`.** Os medidores externos, que servem aos dois apps:
  - o tempo até a janela aparecer no `CGWindowList`;
  - o mouse por `CGEvent`, com leitura só de propriedades do device via Core Audio.

## Fluxo em runtime

**Threads:**
- **Main (GPUI).** Um `Store` guarda o `Controller` e as entidades do modelo. Um laço
  `cx.spawn` com `timer(100 ms)` chama `tick()`. Só a main altera o modelo.
- **Comandos.** Uma `std::thread` recebe `Command` por `mpsc` e executa as chamadas bloqueantes
  na ordem de chegada. A resposta volta como `Outcome`, com o resultado, `last_timings` e os
  horários de enfileirar e de começar.
- **Volume.** Uma thread própria guarda o último valor pedido em `Mutex<Option<f32>>` e espera
  num `Condvar`. Grava só o último valor e pode correr em paralelo com os comandos, porque o
  mutex do core protege o hardware.
- **Carga.** Quatro threads leem as linhas na ordem da fila: tags, miniatura e `probe_track`. A
  capa grande e o fundo desfocado da faixa atual têm um carregador à parte, com contador de
  geração, e o resultado de outra faixa é descartado.

**O `tick()`, na ordem do `refresh()` do Swift:**
1. Drena as respostas dos comandos e dos carregadores.
2. Lê o `snapshot()`. Se `queue_version` mudou, chama `try_queue_items()`, que nunca espera o
   engine, e sincroniza a carga das linhas.
3. Calcula `display`, `clock`, `queue`, `volume` e `error`, e notifica só a entidade que mudou
   (comparação por `PartialEq`).
4. Se `problem_seq` mudou, põe `last_problem()` no aviso.
5. Com `device_suspect` e sem recuperação em curso, enfileira `Recover`.
6. Aplica a regra do auto-avanço com os predicados do Swift: estado `Finished`, próxima faixa
   existe, nenhum comando em curso e esse índice ainda não avançado.

**Regras portadas do Swift:**
- **Comandos em curso.** O contador sobe ao enfileirar e desce quando a main aplica a resposta.
  Ele bloqueia o auto-avanço e o acompanhamento do volume externo.
- **Next rápidos somam.** A contagem pendente fica num `Mutex<Option<i32>>`. Cliques somam nela
  enquanto o comando espera, e a thread de comandos a zera ao começar.
- **Tocar ou pausar** decide pelo estado lido na hora de executar.
- **Erro de antes do comando.** Um comando bem-sucedido limpa o erro anterior, mas não um
  problema publicado durante o próprio comando (a regra de `problemSeqAtCommandStart`).
- **Slider e volume externo.** O slider segue o volume do device só em Playing ou Paused, sem
  arrasto, sem escrita pendente e sem comando em curso, com tolerância de 0,005.
- **Primeira carga.** Arquivos soltos numa fila vazia selecionam a posição 0 explicitamente,
  para a falha aparecer.
- **Log de transições.** Uma linha por transição, no mesmo formato do Swift ("seleção: espera X
  ms, comando Y ms (open…, release…)"), em stderr e em `~/Library/Logs/hog-player-gpui.log`.

**Diferença conhecida.** O timer do GPUI é um `dispatch_after` sem folga, e o do Swift tem
20 ms. Se as medições 1 e 2 apontarem custo no poll, a troca é por um timer de GCD com folga, e
a troca fica registrada no resultado.

## Erros e saída

- **Falha de comando.** O texto do `PlayerError` vai para o aviso.
- **Perda do device.** O poll marca a suspeita e `Recover` entra na fila. O core desmonta e
  publica o problema, e o aviso aparece. O auto-avanço não dispara, porque a recuperação conta
  como comando em curso.
- **Arrastar sem áudio.** "nenhum arquivo de áudio encontrado".
- **Tag ilegível.** A linha mostra o nome do arquivo, sem aviso.
- **⌘Q e fechar a última janela.** Os dois terminam em `on_app_quit` e depois em
  `controller.shutdown()`, que:
  1. marca o encerramento, e os comandos que ainda não começaram são descartados;
  2. esvazia as cargas pendentes;
  3. chama `player.shutdown()`, que espera o comando em curso pelo lock do core;
  4. grava no log qualquer erro.
- **Ctrl+C, SIGTERM, SIGHUP e SIGQUIT.** Uma thread com `signal-hook` marca
  `quit_requested`, e o próximo tique chama `cx.quit()`. O caminho é o mesmo do ⌘Q, com até
  100 ms de atraso.
- **Pânico.**
  - O perfil release mantém o unwind.
  - Os callbacks do AppKit no GPUI são `extern "C"`, então um pânico ali aborta.
  - Um panic hook tenta o `shutdown()` numa thread, espera no máximo 500 ms e deixa abortar.
    Se o engine estiver travado, o macOS solta o hog, mas o rate fica trocado.
  - A medição 9 mostra qual dos dois caminhos acontece.

## Critérios e medições

**Montagem:**
- MacBook Air M1 na bateria, com brilho fixo e só o Terminal aberto além do app medido.
- Um app por vez, porque o hog é por PID.
- Três rodadas de cada, em ordem alternada; vale a mediana.
- O controle é o app SwiftUI instalado (main `a11fe30`). O protótipo é o `.app` release de
  `make gpui-app`, fora do `/Applications`.
- Janela de 1280×800 pt nos dois.
- Fila com a biblioteca inteira: 1.157 faixas distintas com capa, já baixadas localmente.
- Os dois fazem o poll a 10 Hz.

**Medições:**
1. **Custo parado.** Janela visível, pausado, fila carregada, 60 s sem mexer. Comando: `sudo
   powermetrics --samplers tasks --show-process-energy -i 1000 -n 60`, lendo CPU ms/s, wakeups
   e energy impact do processo. Repete com a janela minimizada. Hipótese: o display link do GPUI
   acorda a main thread a 60 Hz.
2. **Tocando.** Uma faixa 24/96 por 60 s, com as mesmas métricas mais `gpu_power`.
3. **Memória.** `footprint` com a janela vazia, com a fila carregada e preenchida, e depois de
   rolar a fila inteira.
4. **Abertura.** Cinco aberturas frias (depois de `purge`) e cinco quentes, medidas do
   lançamento até a janela aparecer no WindowServer pelo medidor externo. O template App Launch
   do Instruments serve de conferência.
5. **Resposta**, medida pelo medidor externo:
   - **Slider.** Do soltar do arrasto até o volume do hardware chegar ao valor final; mais
     hitches e p99 de frame em 5 s de arrasto.
   - **Clique na fila.** Do clique até o device voltar a rodar (`DeviceIsRunningSomewhere`).
   - **Delete.** Dez vezes no meio da fila, conferindo que saiu a linha certa.
   - **Rolagem.** A fila inteira, medindo hitches e p99 de frame.
   - **Encher a fila.** Do soltar da biblioteca até a última linha ter capa e selo. Fica
     registrado à parte, porque os leitores de tag são diferentes.
   - Se o HAL recusar que outro processo leia um device em hog, a medida cai para os logs de
     transição dos dois apps.
6. **Áudio sob estresse.** Cinco minutos tocando 24/96 enquanto a fila rola, a janela é
   redimensionada, o slider é arrastado e as faixas são trocadas. Critério: zero underrun na
   linha técnica e zero `ProcessorOverload` no Audio System Trace.
7. **VoiceOver**, conferido no Accessibility Inspector. O GPUI roda com a acessibilidade ligada.
   Os itens são:
   - cada linha lê título, artista, duração e selo;
   - os botões têm nome;
   - o slider ajusta pelo teclado;
   - Delete e ⌘O funcionam sem mouse;
   - o aviso é alcançável.
8. **Integração com o Mac:**
   - ⌘O no menu e como atalho (no SwiftUI, hoje, é só atalho do botão da barra);
   - filtro de tipos no painel de abrir;
   - vários arquivos e pastas arrastados do Finder;
   - barra transparente e tamanho mínimo;
   - tooltip no selo;
   - Delete e espaço;
   - ⌘Q e Ctrl+C devolvem o device, com o hog em −1 e o rate original.
9. **Pânico.** Um `panic!` injetado num clique, atrás de uma feature, só no protótipo, para ver
   em que estado o device fica.
10. **Build.** Tempo do release limpo, tamanho do `.app`, número de crates e se exige o Xcode
    completo.

As medições 1, 2, 5, 6, 8 (a parte do ⌘Q) e 9 tomam o device. Os itens 7 e 8 são sim ou não,
com observações.

**Regra de decisão**, combinada antes de medir:
- **Eliminam o GPUI:**
  - qualquer underrun sob estresse;
  - VoiceOver que não lê a fila;
  - custo parado ou tocando acima do SwiftUI além da variação entre rodadas.
- O resto classifica e entra no resultado, mas não decide sozinho.

## Testes e gate

**TDD:** teste vermelho antes do código, e sabotagem provando que cada teste pega o defeito.

- **Controlador, com o `FakePlayer`.**
  - O falso tem fila, portões para segurar um comando, erros programáveis, volume `None`, mudo,
    `problem_seq` e `device_suspect`.
  - Os cenários vêm do `ViewModelTests.swift`, e a lista exata sai no plano:
    - ordem serial dos comandos;
    - Next somados;
    - auto-avanço uma vez por índice, nunca com comando em curso nem depois da perda;
    - recuperação enfileirada uma vez;
    - regra do erro de antes do comando;
    - volume gravando só o último valor e voltando ao real se falhar;
    - acompanhamento do volume externo;
    - remoção e seleção por id com a fila defasada;
    - primeira carga;
    - linhas na ordem da fila, com as antigas descartadas;
    - shutdown descartando o que não começou.
- **Equivalente dos `ObservationTests`.** O `tick()` devolve quais partes mudaram. Um tique que
  só avança o relógio marca só `clock`. A sabotagem devolve o relógio ao `display`.
- **Portes puros.** `DisplayState`, `Quality`, `TimeFormat`, `PlayerLayout` e `AudioFiles`,
  com os testes Swift correspondentes portados.
- **`media/`.**
  - Tags e capa lidas das fixtures com imagem embutida que o `make fixtures` já gera
    (`meta_fixture.flac`, `.m4a` e `.mp3`).
  - A miniatura sai no tamanho certo.
  - Os testes do Pawse para a imagem do fundo desfocado.
- **UI.** Poucos `#[gpui::test]`: Delete remove o id selecionado, o clique seleciona e soltar
  arquivos chama o `add`. Se o `test-support` da v1.21 não funcionar fora do workspace do Zed,
  isso vira verificação manual registrada.

**`make gpui-test`:**
- roda `cargo fmt --check`, `clippy --all-targets -D warnings` e `cargo test` em `apps/gpui`;
- reprova se `git diff main -- rust/` não estiver vazio;
- reprova se qualquer pacote da árvore do core (nome, versão e features) divergir do `rust/Cargo.lock`, salvo as exceções listadas com motivo em `apps/gpui/tools/core_version_exceptions.txt`; `coreaudio-sys` e `uniffi` nunca são exceção.

O `make test` atual roda junto a cada passo e continua intocado.

**Build:**
- `make gpui` gera o binário release.
- `make gpui-app` monta o `.app` em `apps/gpui/build/`, com bundle id próprio e sem instalar.
- Os alvos existentes do Makefile não mudam.

## Ordem de construção

Cada passo termina com o gate verde e um aviso ao usuário.

0. **Spike de resolução, sem device.**
   - O Zed v1.21.0 resolve e compila fora do workspace dele, e uma janela vazia abre.
   - Fica registrado o tempo de build e as versões fixadas.
   - Confere se o `objc2` em uso tem o use-after-free do release (objc2 #861).
   - Confere se `gpui_platform::application()` liga a acessibilidade e quais são os nomes da API
     de rótulos.
   - Se algo reprovar, o trabalho para e o problema vai ao usuário.
1. Portes puros.
2. `media/` e as fixtures.
3. Controlador.
4. Esqueleto da UI: janela, barra, menus, duas colunas, `Store` com o tique, faixa atual,
   relógio, transporte e slider.
5. Fila.
6. Capa, fundo com o cruzamento, arrastar do Finder, painel de abrir e aviso.
7. Saída: ⌘Q, sinais, panic hook e log.
8. Rótulos de acessibilidade.
9. **Uso real com o device**, com aviso antes: tocar, remover duas linhas do meio e desconectar
   o fone.
10. Medidores do `bench/` e as medições de 1 a 10, com aviso antes de cada uma que toma o
    device.
11. Doc de resultado em `docs/superpowers/specs/`, enxuto e sem tabelas, com os números, o sim
    ou não de cada item e o veredito pela regra. O doc do vitral recebe o que o passo 0 provar.

## Riscos conhecidos

- A API do GPUI muda sem aviso. Fixar a tag protege o protótipo, mas o porte para outra tag
  custa trabalho.
- Acessibilidade experimental: o próprio Zed 1.21 roda com ela desligada, salvo
  `ZED_EXPERIMENTAL_A11Y=1`.
- `objc2` 0.6.4 tem um use-after-free em release (#861). A correção estava no main em
  25/09/2026, sem release. O passo 0 decide se o protótipo fixa um commit corrigido.
- Compilar os shaders Metal exige o Metal Toolchain do Xcode, que faltava nesta máquina em
  27/09. Ele é baixado com `xcodebuild -downloadComponent MetalToolchain`, só com o ok do
  usuário. A alternativa `runtime_shaders` compila os shaders a cada abertura e distorceria a
  medição 4.
- O clone do Zed ocupa cerca de 527 MB no cache do cargo.
