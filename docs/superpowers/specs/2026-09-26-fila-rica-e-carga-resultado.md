# Fila rica e carga ao tocar: resultado

Data: 2026-09-26
Plano: `~/.claude/plans/o-seguinte-estou-sorted-lighthouse.md`
Estado: no working tree, sem commit, junto com a fila e o tuning que também estão pendentes.

## O que mudou

**Fila.** Cada linha mostra:
- miniatura da capa;
- título e "Artista — Álbum";
- selo de qualidade (`24/96`), com o rótulo completo no tooltip;
- duração.

A carga começa quando a faixa entra na fila, na ordem da fila, com no máximo 4 simultâneas.
Isso revoga a decisão da spec da playlist de mostrar só o nome do arquivo.

O selo vem de `probe_track`, que abre o arquivo com o mesmo `AudioSource::open` do engine:
mostra o formato que vai chegar ao DAC, não o que a extensão sugere. A lista guarda só a
miniatura (96 px, via ImageIO), nunca a capa inteira.

**Caminho do play.**
- Os metadados saíram da cadeia de comandos: o Next não espera mais a leitura do AVFoundation.
- O título aparece na hora, vindo da linha da fila.
- A capa anterior fica na tela até a nova chegar, em vez de piscar o placeholder.
- Next e Previous seguidos viram uma carga só (`navigate(steps)`). A regra de somar os passos
  fica em `queue.rs`, junto com as outras regras da fila.
- O `refresh` só publica o que mudou e lê a fila com `try_lock`: um comando segurando o
  engine não congela mais a janela.
- O timer de 10 Hz roda também no modo de rastreamento, então o relógio não para mais
  enquanto o usuário arrasta o volume.
- A capa grande é decodificada fora da main e limitada a 1200 px.

**Medição.** `last_timings()` expõe o tempo de cada etapa do último comando: `open`,
`release`, `reopen`, `acquire`, `volume`, `prefill` e `start`. Um mutex próprio guarda
esses tempos, fora do lock do engine. O `Logger` registra a espera na fila de comandos, a
duração do comando e as etapas:

`/usr/bin/log show --last 5m --predicate 'subsystem == "local.hogaudio.player"'`

O `log` do zsh é outro comando, daí o caminho completo.

## Bit-perfect

Nada tocou IOProc, ring, decodificador, negociação ou a ordem hog → rate → formato → volume.
Provas rodadas depois das mudanças:
- `make verify` em 24/96: 1.152.000 amostras idênticas ao oráculo ffmpeg;
- `make verify` em 16/44.1: 529.200 amostras idênticas;
- `make verify` num trecho real de 24/96: 1.536.000 amostras idênticas;
- `make verify-pause`: fluxo idêntico com e sem pausa;
- `make verify-volume`: amostras idênticas a 20% e 90%.

O lint do clippy 1.98.1 (`chunks_exact` → `as_chunks`) mexeu em `decode_float32`. Essa função
é do modo afinado, não do bit-perfect, e a troca é equivalente byte a byte.

## Medições

Saída embutida do Mac, com 44.1 kHz em repouso. Trechos locais de 8 s. Um DAC externo
costuma demorar mais para travar num rate novo, então estes números são piso.

Linha de base (só a medição, sem as otimizações):
- play: 98 ms, sendo acquire 14, volume 36, prefill 6 e start 40;
- auto-avanço 24/96 → 24/96: 122 ms, sendo release 51, acquire 15, volume 36 e prefill 12;
- auto-avanço 24/96 → 24/44.1: 107 ms, sendo release 47, acquire 2 e volume 36;
- metadados: 8 a 12 ms.

Depois:
- play: 103 ms;
- auto-avanço no mesmo formato: 119 ms;
- troca de formato: 109 ms;
- metadados: 1 a 2 ms, porque a carga da fila já tinha trazido os arquivos para o cache.

As etapas do device não mudaram, como esperado: o plano não mexeu nelas. Com arquivo local,
a espera por metadados era pequena, e o ganho do Swift não aparece nesses três cenários.
Ele aparece em outros lugares:
- cliques repetidos viram uma carga só;
- o título aparece sem atraso;
- a janela não congela;
- num arquivo lento de ler, o Next não espera mais.

## Onde está o tempo agora

Cerca de 65 ms de cada troca de faixa são `sleep` fixo de 30 ms na confirmação de volume
(`write_volume_confirmed`, `device.rs:377-391`). Acontece uma vez no `restore` da faixa
que sai e outra no `apply_volume` da que entra. O teto de cada uma é 600 ms.

O resto é o ciclo completo de soltar e tomar o device, que acontece mesmo entre faixas de
formato idêntico.

## Frentes adiadas

- **Manter o hog entre faixas.** Com o mesmo formato, trocar só produtora e ring; com
  formato diferente, trocar o rate direto, sem voltar ao original. Isso elimina o `release`,
  o `acquire` e as duas confirmações de volume: hoje cerca de 100 ms na saída embutida, e até
  2 × 2 s de `wait_for_rate` num DAC lento. Revoga a decisão da spec da playlist, então pede
  ADR e teste de hardware.
- **Confirmação de volume por listener de propriedade**, no lugar do `sleep` de 30 ms. É o
  maior custo fixo medido.
- **Erro de leitura tratado como fim de arquivo** (`source.rs:223`): a faixa é cortada e o
  player pula sem marcar falha.
- **Dois `open` por carga** (`engine.rs:230` e `:532`) e **carga dupla da primeira faixa**
  (`api.rs:87` + `PlayerViewModel` em `add`). Localmente custa pouco (0,3 a 2 ms por
  abertura).
- **Pasta em streaming.** Se a biblioteca voltar a ficar só na nuvem, cada início de faixa
  paga cerca de 2,2 s. Medido no Google Drive: ele atende leitura parcial (blocos de 4 MB,
  cerca de 4 MB/s), e com o cache quente o início custa 0 s. A saída é aquecer os primeiros
  MB da próxima faixa enquanto a atual toca.

## Testes

- 152 testes Rust (+10), com 14 de hardware ignorados (+1).
- 68 testes Swift (+22).
- Todo teste novo foi visto falhando antes de passar: contra o stub ou com o código
  sabotado. O limite de 4 cargas, a coalescência, o descarte de metadado antigo, o `refresh`
  sem republicar e a leitura sem bloqueio foram provados por sabotagem.
- O teste que garante que o Next não espera metadados usa espera com prazo. O `.timeLimit`
  do Swift Testing não solta uma continuation presa, e a suíte travava em vez de reprovar.
