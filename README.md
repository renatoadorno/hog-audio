# hog-audio

Reprodutor de linha de comando para macOS que entrega o arquivo ao DAC pelo caminho digital
mais curto que o sistema permite: sem resample, sem mixer e sem nenhum outro processo
dividindo o conversor.

```
make
./build/hog-audio musicas/faixa.flac
```

## O que ele faz

1. Lê o formato real do arquivo com o decodificador do próprio macOS — FLAC, ALAC, WAV,
   AIFF, CAF, AAC e MP3, sem biblioteca externa.
2. Enumera o que o device de saída aceita e decide se existe caminho sem perda. **Se não
   existir, recusa tocar** em vez de resamplear em silêncio.
3. Toma o device em modo exclusivo (`kAudioDevicePropertyHogMode`). Enquanto toca, nenhum
   outro aplicativo consegue emitir som.
4. Trava o device no sample rate do arquivo. O DAC muda fisicamente de frequência.
5. Alimenta um `AudioDeviceIOProc` direto, sem passar pelo mixer do CoreAudio.
6. Devolve o device ao estado original ao terminar, inclusive com Ctrl+C.

```
$ ./build/hog-audio musicas/faixa.flac
arquivo  : flac — 96000 Hz / 24 bits / 2 canais
device   : Alto-falantes (MacBook Air)
rate atual: 96000 Hz
rates    : 44100, 48000, 88200, 96000
negociado: 96000 Hz / 32 bits float
exclusivo: sim (hog mode)
físico   : 96000 Hz / 32 bits float / 2 canais
callback : 96000 Hz / 32 bits float / 2 canais [travado igual ao físico]
tocando  : 203.5 s — Ctrl+C interrompe
```

`--info` mostra a mesma negociação sem tocar no device — útil para conferir um arquivo antes
de interromper o áudio do sistema.

## Volume

A saída de fone do Mac entrega tensão suficiente para machucar tanto o ouvido quanto o fone
quando fica no máximo. O volume é ajustado **depois de tomar o device e antes de qualquer som
sair**, e devolvido ao valor anterior no fim.

```
--volume 35        35% do controle, como o slider do sistema
--volume -18dB     atenuação exata de 18 dB
--max-volume 60    muda o teto de segurança (padrão 50%)
```

Sem `--volume`, o volume só é tocado se estiver acima do teto — assim esquecer a flag não
significa levar o volume cheio no fone.

Os decibéis vêm do amplificador, não de uma fórmula: pedir `-30dB` resulta em exatamente
−30,0 dB no device. O número exibido é sempre lido do hardware depois de aplicado, porque a
conversão de escalar para decibéis que o HAL publica **não corresponde** à curva que ele de
fato aplica — ela informaria −50,8 dB onde o device está a −35,1 dB.

Duas armadilhas do Core Audio que este código contorna, ambas observadas neste Mac:

- A primeira escrita de volume após uma reconfiguração de formato é **descartada em
  silêncio**: devolve `noErr` e a leitura seguinte ainda retorna o valor pedido, vindo de
  cache. Sem confirmar com uma releitura atrasada, o volume simplesmente não valeria — e o
  som sairia no volume anterior. Por isso um pedido explícito que não possa ser confirmado
  aborta a reprodução em vez de tocar.
- Enquanto o device está tomado, o macOS aponta o **"dispositivo de saída padrão" para outro
  device**. Ferramentas que leem "o default" (inclusive `system_profiler`) mostram o volume
  do device errado durante a reprodução.

## Por que float32 continua sendo bit-perfect

O IOProc recebe os frames no *formato virtual* do stream. No áudio interno do Apple Silicon
isso é float32, e o HAL não oferece formato físico inteiro — os quatro formatos publicados
são float32, variando apenas o sample rate.

Isso não compromete a integridade: float32 tem 24 bits de mantissa, então todo inteiro de até
24 bits é exatamente representável, e a escala por 2²³ só altera o expoente. A ida e a volta
entre int24 e float32 devolvem o mesmo valor. O que realmente degradaria o sinal é outra
coisa, e é o que este programa evita:

| Risco | Como é evitado |
|---|---|
| Resample (SRC) | O device é travado no rate do arquivo; sem rate compatível, não toca |
| Mixagem com outros apps | Hog mode exclusivo |
| Volume aplicado em software | O controle usado é o do amplificador, o mesmo do slider do sistema |

Para arquivos acima de 24 bits o programa recusa o formato float32, porque aí a conversão
deixaria de ser exata.

## Proteção contra ruído

Se o formato que o decodificador produz não for exatamente o que o DAC espera, o resultado
não é uma distorção discreta: é ruído branco em volume total, capaz de danificar fone e
audição. Três barreiras existem para que nenhuma amostra chegue ao device nessa condição:

1. O tamanho do frame vem do próprio device, nunca de `bitsPerChannel / 8`. Um formato pode
   carregar amostras de 24 bits em containers de 32, e assumir empacotamento faria o
   decodificador escrever com um passo e o callback ler com outro.
2. Depois de configurar a entrega, o formato que o decodificador de fato assumiu é relido e
   comparado com o que o device espera. Divergiu, o programa para antes de tocar.
3. No callback, todo byte enviado é dado do arquivo ou zero. Buffer curto, canal excedente ou
   pedido maior que o previsto resultam em silêncio, nunca em memória não inicializada.

## O que acontece se o processo morrer

O modo exclusivo é associado ao PID pelo `coreaudiod`, que o devolve quando o processo
termina — verificado inclusive com `kill -9`: o áudio do sistema volta sozinho, sem precisar
reiniciar serviço nenhum.

| Saída | Modo exclusivo | Sample rate |
|---|---|---|
| Fim da faixa, Ctrl+C, `SIGTERM`, `SIGHUP`, `SIGQUIT` | devolvido pelo programa | restaurado |
| `SIGKILL`, crash | devolvido pelo sistema | permanece no valor em uso |

Ou seja: o Mac não fica mudo em nenhum caso. No caminho abrupto o que sobra é o sample rate
alterado, corrigível tocando qualquer outra coisa ou pelo Configuração de Áudio e MIDI.

## Limites conhecidos

- **Detecção de impedância**: o jack que ajusta a tensão para fones de alta impedância existe
  no MacBook Pro 14"/16" (2021) em diante, no MacBook Air M2 e no Mac Studio. No MacBook Air
  M1 (2020) esse recurso não existe — o rate lock e o modo exclusivo funcionam normalmente,
  mas não há ganho de tensão para fones pesados.
- **Sem resample, por escolha**: um arquivo de 192 kHz num device que vai até 96 kHz é
  recusado, com a lista de rates suportados.
- **Uma faixa por vez**, sem playlist, seek ou pausa.
- O device é escolhido no início da reprodução. Plugar um fone no meio da faixa não migra o
  áudio, porque o device já está tomado.

## App gráfico (HogAudio.app)

O mesmo núcleo ganha uma janela: `apps/player` é um pacote Swift que fala com o engine Rust
por FFI. Rodado como binário solto pelo Terminal ele **não abre janela** — sem `Info.plist`, o
macOS trata o processo como segundo plano. O `.app` existe só por isso.

```
make app
open apps/player/HogAudio.app
```

Ou já com uma faixa:

```
make run-app FILE=musicas/faixa.flac
```

Não há entitlement de sandbox de propósito: hog mode não sobrevive a ele, o que fecha a porta
da App Store — não é objetivo deste projeto. Também não há assinatura de código nem ícone
customizado (YAGNI).

**O que o player faz:** abrir um arquivo (painel ou arrastar), tocar, pausar, ajustar o volume
e mostrar título, artista, álbum e capa quando embutida.

**O que ele não faz:** sem seek, sem lista de reprodução — uma faixa por vez.

**Comportamentos que são decisão, não defeito:**

- O Mac fica mudo enquanto o player segura o device — **inclusive pausado**. O hog mode só é
  liberado quando a faixa termina ou o player fecha, nunca só por pausar.
- Encerrar o processo à força (force-quit, `kill -9`) devolve o modo exclusivo pelo sistema,
  mas deixa o **sample rate trocado**; corrige tocando qualquer outra coisa ou pelo
  Configuração de Áudio e MIDI.
- Tirar o fone durante a reprodução **não é tratado nesta versão**: o device já foi tomado no
  início da faixa, o relógio congela e a única saída é fechar o player.

## As três provas de bit-perfect

É o que diferencia este projeto de um tocador comum: qualquer alteração no caminho do áudio
tem que continuar batendo nas três.

```
make verify FILE=testdata/t96_24.flac BITS=24  # C++ e Rust batem, amostra a amostra, com o PCM do ffmpeg
make verify-pause FILE=testdata/t96_24.flac    # pausar no meio não descarta nem duplica byte do ring buffer
make verify-volume FILE=testdata/t96_24.flac   # volume muda no device, nunca nas amostras entregues
```

## Estrutura

```
src/format_negotiation.*  decide rate e formato — puro, sem Core Audio, coberto por testes
src/volume.*              interpreta o volume pedido e o teto — puro, coberto por testes
src/ring_buffer.hpp       fila sem locks entre o decodificador e a thread de tempo real
src/audio_source.*        decodificação via ExtendedAudioFile
src/hog_device.*          HAL: modo exclusivo, lock de formato, IOProc, restauração via RAII
src/main.cpp              CLI, orquestração e tratamento de sinal
```

A decisão de "isto pode tocar sem perda?" fica isolada do hardware de propósito: é a regra que
mais importa e a única que dá para verificar sem plugar um fone.

## Desenvolvimento

```
make test        # testes do núcleo puro
make test-asan   # os mesmos testes sob AddressSanitizer e UBSan
make info FILE=musicas/faixa.flac
make play FILE=musicas/faixa.flac
```

O IOProc roda em thread de tempo real: dentro dele só existem `memcpy` e operações atômicas —
nada de alocar, travar ou imprimir. Alterações naquele caminho devem ser verificadas com
`make test-asan`.
