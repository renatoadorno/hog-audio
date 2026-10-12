# Roadmap do player

Escrito em 11/10/2026. É um documento vivo: cada item ganha data quando for decidido, feito ou
descartado. O desenho de cada fase sai depois, num spec próprio em `docs/superpowers/specs/`.

## Onde estamos

Hoje o hog-audio é um bom reprodutor, mas ainda não é um player completo.

O que já existe:
- Reprodução bit-perfect em hog mode, com o DAC travado no rate e no bit depth do arquivo, e o
  volume no hardware.
- Uma tela de reprodução em duas colunas:
  - à esquerda: capa grande, fundo desfocado, título, artista e álbum, relógio, transporte,
    volume e a linha técnica (rate, bits, formato, hog);
  - à direita: a fila, com miniatura, título, artista e álbum, selo de qualidade e duração.
- A fila de reprodução, que hoje é a única lista do app:
  - entra por "Abrir" (⌘O) ou arrastando do Finder;
  - pastas entram recursivamente, em ordem de caminho;
  - Delete remove uma faixa, "Limpar" esvazia;
  - o auto-avanço passa à faixa seguinte;
  - o device perdido é recuperado.

O que o core recusa hoje (`rust/src/source.rs`, `rust/src/format.rs`):
- **Formatos com perda fora de AAC-LC e MP3** (HE-AAC, Opus e outros): recusados com o nome do
  codec na mensagem. AAC-LC e MP3 tocam desde 11/10/2026 (Fase 0.1); até lá, todo formato com
  perda era recusado, por decisão de 08/2026.
- **Rate que o device não suporta:** a faixa é recusada, porque "Reproduzir exigiria resample".
- **Arquivo acima de 24 bits** num device que só tem float32, e arquivo com mais canais do que o
  device.
- **Tocar sem o modo exclusivo:** o core sempre toma o device em hog mode, e não há como
  desligar.

O que falta para virar um player completo:
- **Nada persiste:** fechar o app perde a fila, e as músicas precisam ser adicionadas de novo a
  cada abertura.
- **Não há biblioteca:** não dá para ver a coleção por álbum, artista ou gênero, nem buscar.
- **Não há playlists.**
- **A fila é mínima:** só se acrescenta ao fim, sem "tocar a seguir", sem reordenar, sem
  aleatório e sem repetição.

## Princípios que não mudam

- **Bit-perfect continua sendo a identidade do player.** O caminho exclusivo e sem perda
  continua intacto e verificado (`make verify*`). A regra das exceções foi decidida em
  11/10/2026 ([ADR 0002 do core](../rust/docs/adr/0002-arquivo-com-perda-sem-perda-do-app.md)):
  o app nunca altera uma amostra do que o decodificador entrega, e a perda aceita é só a do
  arquivo. O player diz sempre na tela quando não está bit-perfect: arquivo com perda e, quando
  existirem, modo compartilhado ou resample.
- **Fora, mesmo assim:** equalizador, ReplayGain aplicado ao sinal e crossfade.
- **Não existe gapless no modo exclusivo.** Cada troca de faixa solta e retoma o device, e pode
  trocar o rate do DAC. A UI não promete o que o core não entrega (ver
  `docs/superpowers/specs/2026-08-19-playlist-design.md`).
- **A interface continua em SwiftUI.** O protótipo em GPUI de 11/10/2026 reprovou no custo com
  o app parado (`docs/superpowers/specs/2026-09-27-prototipo-gpui-resultado.md`).
- **Sem tecnologia web:** nada de webview, Tauri ou Electron.
- **A tela de reprodução atual é boa e fica.** As features novas entram em volta dela; o que
  muda nela é só o necessário para caber na navegação nova.

## Fase 0: core da reprodução (antes de tudo)

Decidido em 11/10/2026: mudar duas decisões de origem do core antes da biblioteca. As duas
mexem no `rust/` e são analisadas com o agente do core (`agente-core-audio`). Cada uma vira um
spec e um plano. Uma ADR registra a mudança, porque a regra de hoje é "sem caminho sem perda,
o player recusa" (README, seção "O que ele faz").

Reduzido em 11/10/2026: a 0.1 vira só AAC e MP3, e a 0.2 fica para depois dela. O escopo vale
para o device do usuário, um MacBook Air M1, cujo fone e caixas só publicam float32; outros DACs
ficam desconsiderados por enquanto.

### 0.1 Tocar outros tipos de áudio

O que muda: arquivos que hoje são recusados passam a tocar, e a tela mostra por que aquela
reprodução não é bit-perfect.

As recusas de hoje têm dois motivos diferentes, conferidos com `hog-audio --info` em 11/10/2026.

- **O limite é do device, não do arquivo.**
  - Exemplo: FLAC de 192 kHz no fone do Mac, que aceita só 44.1, 48, 88.2 e 96 kHz.
  - O arquivo é sem perda e toca bit-perfect num DAC que aceite 192 kHz.
  - No fone, só toca com resample.
  - A recusa sai da negociação, em `format.rs`.
- **A regra é sobre o arquivo.**
  - Exemplo: AAC e MP3.
  - Formato com perda não tem profundidade de bits, porque o decodificador entrega float.
  - O core recusa antes de olhar o device, em `source.rs`, e a recusa vale para qualquer device.

Decidido em 11/10/2026 (spec em
`docs/superpowers/specs/2026-10-11-audio-com-perda-design.md`):
- **Entram AAC-LC e MP3, no modo exclusivo.** O float32 do decodificador vai intacto ao device
  float32, no rate do próprio arquivo, sem resample e sem arredondamento. Isso vale para `.m4a`
  com AAC-LC, como a maior parte da biblioteca do usuário (305 a 339 kbps). `.m4a` com ALAC já
  tocava e não muda.
- **O FLAC e os outros formatos sem perda não mudam.**
- **O rate que o device não suporta continua recusado**, para qualquer formato. No fone do Mac
  isso inclui o FLAC acima de 96 kHz e o AAC ou MP3 em 22,05 ou 32 kHz.
- **A tela mostra codec e bitrate na faixa com perda:** "AAC 339" no selo da fila e
  "48 kHz · AAC 339 kbps" na linha técnica. A API troca `bit_depth: u32` por
  `bit_depth: Option<u32>` e ganha `bitrate_kbps: Option<u32>`.
- **A prova da faixa com perda** compara o dump do player com o `afconvert`, que usa o mesmo
  decodificador da Apple. O ffmpeg não serve de oráculo para AAC e MP3.
- **O README é corrigido junto com o código:** hoje ele diz que o core lê AAC e MP3.

Implementado em 11/10/2026, na branch `feat/audio-com-perda`
(plano em `docs/superpowers/plans/2026-10-11-audio-com-perda.md`).

Para depois, com data de 11/10/2026:
- **Resample:** uma lógica "inteligente", que resampleia só em casos específicos, a decidir. Os
  três caminhos levantados continuam na mesa:
  - liberar só no modo compartilhado, deixando o macOS resamplear;
  - resamplear no core, com aviso na tela;
  - continuar recusando no modo exclusivo, que é o que vale até lá.
- **HE-AAC (`aach`, `aacp`), Opus e outros codecs com perda:** ficam recusados até um teste
  provar o rate e o formato de entrega de cada um. No HE-AAC, o rate declarado pode ser o do
  núcleo, metade do de saída, e o `ExtAudioFile` resamplearia em silêncio.
- **WAV, AIFF e CAF float de 32 bits:** recusados num device float32, embora float32 para float32
  seja exato. É um falso negativo do `preserves_every_bit`, que olha só o bit depth (conferido
  com `--info` em 11/10/2026). O inteiro de 32 bits e o float de 64 são recusados com razão.
- **DACs que só aceitam inteiro:** AAC e MP3 são recusados neles, porque converter exigiria
  arredondar o float do decodificador, com ou sem dither.
- **Arquivo com mais canais do que o device:** continua recusado.
- **AAC com DRM (`.m4p`) e arquivo corrompido:** a faixa termina no erro de decodificação, sem
  mensagem.

### 0.2 Modo exclusivo ligável

O que muda: uma opção para tocar sem tomar o device. O som passa pelo mixer do sistema, e os
outros apps continuam tocando.

Adiada em 11/10/2026: começa depois da 0.1. A análise do código já apontou dois caminhos de
saída para o modo compartilhado:
- o mesmo IOProc sem hog, que é simples, mas toca na velocidade errada se outro app trocar o
  rate nominal no meio da faixa, e não segue a troca de saída (plugar o fone);
- uma output unit (AUHAL), que converte para o que o device estiver fazendo e pode seguir a saída
  padrão, num caminho separado do hog.

Em aberto:
- O caminho de saída no modo compartilhado: sem hog, pelo mixer do macOS, e se o player ainda
  troca o rate nominal do device ou deixa o sistema resamplear.
- O volume: o do hardware, como hoje, ou o do sistema e do app.
- Onde fica a opção: preferência global, por device ou um botão perto do selo "hog". Ela vale
  na próxima faixa ou na hora, tocando.
- O que muda na recuperação de device perdido e no `device_watch`, que hoje assumem que o app é
  dono do device.
- Se o modo compartilhado permite gapless, já que não há troca de rate entre faixas.
- O que acontece com o protótipo GPUI (`apps/gpui`), que usa a API pública do core.

## Fase 1: biblioteca persistente

O objetivo é que uma música adicionada uma vez fique no app.

- **Pastas da biblioteca.** O usuário escolhe uma ou mais pastas, por exemplo
  `~/Music/Masterizados`.
  - O app lembra delas.
  - O app reescaneia as pastas ao abrir e observa as mudanças enquanto está aberto (FSEvents).
  - Música nova aparece sozinha.
- **Arquivos soltos.** Arrastar arquivos que estão fora das pastas os acrescenta à biblioteca,
  além da fila.
- **Índice local**, guardado em disco, com:
  - as tags: título, artista, artista do álbum, álbum, número da faixa e do disco, ano, gênero e
    compositor;
  - o formato: rate, bits, codec e duração;
  - a miniatura da capa, num cache;
  - o caminho e a data em que o arquivo entrou.
- **Leitura de tags ampliada.** Hoje o `Metadata.swift` só lê título, artista, álbum e capa.
- **Arquivo que sumiu**, num disco externo desligado ou apagado: fica marcado como indisponível,
  sem sair da biblioteca nem das playlists.
- **Faixa que o device não toca** sem resample: marcada antes do play, com o `probe_track` do
  core e o device atual.
- **A primeira varredura** de uma biblioteca grande não trava a tela: mostra o progresso e deixa
  tocar enquanto indexa.

Em aberto:
- onde o índice mora: SwiftData, SQLite (GRDB) ou um índice no core em Rust;
- se o core passa a conhecer a biblioteca, ou se ela fica só no app Swift.

## Fase 2: navegar a biblioteca

- **Seções:**
  - Álbuns, numa grade de capas;
  - Artistas, numa lista que leva aos álbuns do artista;
  - Músicas, numa lista com colunas ordenáveis;
  - Gêneros;
  - Pastas, navegando pela árvore do disco, comum em players para quem organiza por pasta;
  - Recém-adicionados.
- **Página do álbum:**
  - capa, artista, ano, duração total e o selo de qualidade (24/96, 16/44.1);
  - as faixas agrupadas por disco;
  - botões Tocar, Aleatório e Adicionar à fila.
- **Página do artista:** os álbuns em grade, as faixas mais tocadas e as participações, como
  artista de faixa num álbum de outro.
- **Busca global** por título, artista e álbum, com resultados agrupados por tipo.
- **Filtro por qualidade:** hi-res, 24 bit e rate mínimo. Isso diferencia um player bit-perfect.
- **Ordenação** por nome, artista, ano, data de entrada e qualidade.

## Fase 3: fila de reprodução completa

- **Ações em qualquer faixa, álbum ou playlist:**
  - Tocar agora, que troca a fila;
  - Tocar a seguir;
  - Adicionar ao fim.
- **Na fila:**
  - reordenar arrastando;
  - remover;
  - limpar;
  - salvar a fila como playlist.
- **Aleatório e repetição**, de uma faixa ou da fila inteira.
- **Histórico:** as faixas tocadas recentemente.
- **Retomar:** a fila e a posição sobrevivem ao fechar o app, e o app abre com a faixa pausada
  onde parou.
- **O core precisa de API nova:** inserir numa posição, mover e trocar a fila inteira. Hoje ele
  só acrescenta, seleciona, remove e limpa. A mudança fica na fila do core, longe do IOProc.

## Fase 4: playlists

- Criar, renomear e apagar playlists.
- Acrescentar faixas, álbuns inteiros ou a seleção.
- Reordenar as faixas e avisar quando uma faixa repete.
- Importar e exportar M3U e M3U8.
- Mais tarde, playlists inteligentes por regra: hi-res, adicionadas nos últimos 30 dias, mais
  tocadas, nunca tocadas.

## Fase 5: o que um player completo também tem

Ordem ainda livre:
- **Teclas de mídia e o Now Playing do macOS** (Central de Controle e tela de bloqueio).
  Conferir antes como isso convive com o hog mode.
- **Mini player:** uma janela compacta ou um item na barra de menus.
- **Favoritos e contagem de reproduções**, que alimentam as playlists inteligentes.
- **Editar tags** de uma faixa ou de um álbum.
- **Atalhos de teclado** para a navegação e a busca.

## Fora, ou só muito depois

- Equalizador, ReplayGain e crossfade, que quebram o bit-perfect.
- Gapless, que é impossível sem reescrever o core.
- Streaming (Qobuz e outros) e scrobble do Last.fm, que ficam para depois de a biblioteca local
  estar pronta.

## O que isso muda na interface

A janela atual é uma tela só: a reprodução com a fila ao lado. A biblioteca e as playlists pedem
navegação, e a tela de reprodução passa a dividir espaço com elas. As perguntas que o design
precisa responder antes do código:

- **Como navegar:** barra lateral (Biblioteca, Playlists), abas no topo ou outra forma.
- **Onde fica a reprodução enquanto se navega:**
  - uma barra de transporte fixa, embaixo ou em cima, que abre a tela atual inteira;
  - a tela atual numa coluna que encolhe;
  - alternar entre "biblioteca" e "tocando agora".
- **Onde fica a fila:** no painel à direita, como hoje, num painel lateral que abre e fecha, ou
  numa tela própria.
- **Como fica a página do álbum,** que é a tela mais usada num player de álbuns.
- **Como o fundo desfocado e a capa grande** convivem com as telas novas.
- **Onde ficam o modo exclusivo e os selos da Fase 0:** o botão de ligar e desligar o hog, e
  como a tela avisa "com perda", "resample" e "compartilhado".

## Próximo passo

Primeiro, a Fase 0.1 com o `agente-core-audio`: a spec de AAC e MP3 está escrita
(`docs/superpowers/specs/2026-10-11-audio-com-perda-design.md`), e o próximo passo é o plano. A
0.2 vem depois.

Em paralelo, sem código, segue o protótipo visual da interface
(https://claude.ai/artifact/8FixeuGM4QkPp8km6H9wWT), com opções lado a lado para escolher e
refinar:
- a disposição da janela;
- a navegação;
- a biblioteca;
- a página do álbum;
- a fila;
- as playlists.

Depois de escolhido o desenho, cada fase vira um spec e um plano. A implementação continua em
SwiftUI.
