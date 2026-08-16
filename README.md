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
| Volume aplicado em software | Não mexemos no volume; no áudio interno do Mac ele age no amplificador |

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

## Estrutura

```
src/format_negotiation.*  decide rate e formato — puro, sem Core Audio, coberto por testes
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
```

O IOProc roda em thread de tempo real: dentro dele só existem `memcpy` e operações atômicas —
nada de alocar, travar ou imprimir. Alterações naquele caminho devem ser verificadas com
`make test-asan`.
