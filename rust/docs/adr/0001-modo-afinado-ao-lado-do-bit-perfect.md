# 1. Modo afinado ao lado do bit-perfect, não no lugar dele

Data: 2026-09-08

## Status

Aceito.

## Contexto

O projeto existe para entregar o arquivo ao DAC pelo caminho digital mais curto que o sistema
permite. O README abre com isso, e o core cumpre literalmente: o `ExtAudioFile` decodifica
**no formato físico do device**, o ring buffer guarda esses bytes, e o `io_proc` os copia. Não
existe ponto no caminho onde uma amostra seja lida como número.

Surgiu a necessidade de aplicar uma curva de resposta em frequência escolhida pelo ouvinte —
um *tuning*, no sentido em que a palavra é usada para fones. Isso é, por definição,
incompatível com bit-perfect: aplicar um filtro é mudar cada amostra.

## Decisão

Dois modos de reprodução, escolhidos por faixa, com o modo bit-perfect **inalterado**.

O que separa os dois é uma decisão só: **qual formato o decodificador entrega**.

| | bit-perfect | afinado |
|---|---|---|
| formato de entrega | o formato físico do device | float32 |
| produtora | `read` → `ring` | `read` → filtro → requantização → `ring` |
| ring buffer | o mesmo | o mesmo |
| `io_proc` | cópia | cópia |
| hog mode, rate travado | sim | sim |

Consequências que decorrem disso, e que foram escolhidas de propósito:

- **O `io_proc` e o ring buffer não mudam.** Todo o processamento vive na thread que
  decodifica, onde alocar e usar FFT não viola disciplina de tempo real.
- **Nada de "sempre float com bypass".** Um caminho único que processasse sempre e desligasse
  o filtro no modo bit-perfect faria as amostras passarem por conversão em float mesmo quando
  ninguém pediu — o modo bit-perfect deixaria de ser literal, e passaria a depender de um
  `if`. Os dois modos são caminhos separados no código.
- **Trocar de modo reinicia a faixa.** O formato de entrega é decidido na partida do stream.
  Trocar com o device tocando significaria mudar o formato debaixo do `io_proc`. Retomar da
  posição atual depende de `ExtAudioFileSeek`, que ainda não existe; até lá, `set_tuning` só
  vale a partir do próximo `play`.
- **A negociação de formato não muda.** O modo afinado usa o formato que a negociação
  escolheu — inteiro, quando o device prefere inteiro — e requantiza com dither TPDF. Fazer a
  negociação preferir float32 no modo afinado seria uma segunda decisão, e não é necessária.

### Fase mínima, não linear

O filtro é um FIR de fase mínima, desenhado pelo cepstro real. Fase linear teria pré-eco
audível como um sopro antes do transiente, e latência de metade do comprimento do filtro.
Fase mínima concentra a energia no início do impulso, que é o que todo EQ analógico faz.

### O preamp não é opcional

Uma curva com ganho positivo satura o fundo de escala. A curva é normalizada no desenho do
filtro — pico em 0 dB, tudo o mais atenuado — e não na entrada, para que o preamp continue
visível como o número que diz quanto a curva pediu além do que cabe.

## Alternativas descartadas

**Cascata de biquads paramétricos.** Não reproduz as ondulações finas de uma curva medida sem
virar um problema de ajuste por otimização. O FIR reproduz qualquer forma direto dos pontos.

**Biblioteca de FFT.** O núcleo da afinação é testável sem Core Audio, e uma dependência
custaria essa pureza sem devolver nada: a radix-2 própria cabe em ~80 linhas, é conferida
contra a DFT ingênua, e desenha o filtro inteiro em 10 ms.

## Consequências

O README deixa de descrever o comportamento completo do binário com uma frase só: passa a
haver um modo em que a saída **não** é bit-perfect, sinalizado na saída do programa em toda
execução (`afinação : desligada — saída bit-perfect`, ou a curva e o preamp).

O contador de clipping entra no `SharedStatus` ao lado dos underruns. São falhas de natureza
diferente — underrun é alimentação, clipping é distorção — e nenhuma das duas interrompe a
reprodução, então sem contador nenhuma das duas seria percebida.
