<p align="center">
  <img src="logos/hog-icon-dark.svg" width="128" alt="">
</p>

<h1 align="center">hog-audio</h1>

<p align="center">
  Player de música hi-res, exclusivo para macOS.<br>
  Bit-perfect: seus FLAC e ALAC tocam exatamente como estão no arquivo, na resolução original,
  com o DAC dedicado só à sua música.
</p>

<p align="center">
  <strong>v0.1.0</strong> · macOS 14 ou mais novo · Apple Silicon · licença MIT
</p>

<p align="center">
  <img src="docs/images/player.png" width="860"
       alt="A janela do hog-audio com o álbum Sugar, de Robin Schulz, em FLAC 24 bits / 96 kHz: a capa e o transporte à esquerda, a fila de 15 faixas à direita">
</p>

## O que ele faz

- **Toca em modo exclusivo.** Toma o device de saída em hog mode
  (`kAudioDevicePropertyHogMode`): enquanto toca, nenhum outro app consegue usar o conversor.
- **Trava o DAC no rate do arquivo.** O conversor muda fisicamente de frequência para a da
  faixa. Se o device não aceita esse rate, o player recusa tocar em vez de resamplear em
  silêncio.
- **Não passa pelo mixer.** Alimenta um `AudioDeviceIOProc` direto, com os bytes que o
  decodificador entregou.
- **Ajusta o volume no hardware.** O controle é o do amplificador, o mesmo do slider do sistema,
  e nenhuma amostra é alterada. Um teto de segurança protege o ouvido e o equipamento ligado à
  saída, seja fone, caixa ou DAC externo.
- **Toca AAC e MP3 sem acrescentar perda.** O float32 que o decodificador da Apple entrega vai
  intacto ao device; a única perda é a da compressão
  ([ADR 0002 do core](rust/docs/adr/0002-arquivo-com-perda-sem-perda-do-app.md)).
- **Mantém uma fila.** Cada faixa mostra capa, título, artista, álbum, duração e um selo de
  qualidade ("24/96", "16/44.1", "AAC 256"). Ao fim de uma, passa para a seguinte.
- **Devolve o device como estava.** Ao terminar, restaura o rate, o formato, o volume e solta o
  modo exclusivo, inclusive com Ctrl+C.

A tela mostra sempre o que chega ao DAC: na linha técnica, "96 kHz · 24 bit · flac · hog ativo"
num FLAC, ou "48 kHz · AAC 339 kbps · hog ativo" numa faixa com perda.

## Por que "hog"

O hog-audio toca pelo próprio sistema de áudio do macOS, o Core Audio, sem driver nem biblioteca
de terceiros. O som sai pelo conversor do Mac, que alimenta a saída de fone e as caixas, ou por um
DAC externo que o macOS reconheça. Os testes até aqui foram feitos na saída interna de um MacBook
Air M1.

E ele toca sozinho. O nome vem do *hog mode* (`kAudioDevicePropertyHogMode`), o modo exclusivo do
Core Audio: em inglês, *to hog* é tomar algo só para si. É o que o player faz com o DAC enquanto
toca, para que nenhum outro app mexa no som que chega a ele.

## Formatos

- **Bit-perfect:** FLAC, ALAC, WAV, AIFF e CAF de até 24 bits, em qualquer rate que o device
  aceite.
- **Com perda só do arquivo:** AAC-LC e MP3, quando o device tem um formato float32 no rate do
  arquivo. As saídas internas do Mac têm.
- **Recusados, com o motivo na tela:**
  - rate que o device não suporta, como FLAC de 192 kHz ou MP3 de 22,05 kHz no fone do Mac;
  - HE-AAC, Opus e os outros codecs com perda;
  - inteiro de 32 bits ou float de 64 bits num device float32, que não os comporta sem perda;
  - arquivo com mais canais do que o device.

## Instalar

Ainda não há binário pronto: o app é montado a partir do código.

Requisitos:
- macOS 14 ou mais novo, testado num MacBook Air M1;
- Rust 1.88 ou mais novo, testado no 1.98;
- toolchain do Swift 6 (Xcode ou Command Line Tools);
- ffmpeg, só para os testes e as provas (`brew install ffmpeg`).

```
make install-app
```

Monta o app, instala em `/Applications` (fechando com segurança uma versão aberta) e o abre.
`make install-app INSTALL_DIR="$HOME/Applications"` instala só para o usuário atual, e
`OPEN_APP=0` não abre no fim.

O app não tem sandbox de propósito, porque o hog mode não sobrevive a ela. Isso fecha a porta
da App Store, que não é objetivo deste projeto.

## Usar

- **Abrir músicas:** ⌘O (arquivos ou pastas, que entram recursivamente), arrastar do Finder para
  a janela, ou "Abrir com → HogAudio" no Finder.
- **Abrir com duplo clique:** no Finder, "Obter Informações" num arquivo → "Abrir com" →
  HogAudio → "Alterar Tudo…", uma vez por tipo de arquivo.
- **A fila:** o que entra vai para o fim; com a fila vazia, a primeira faixa fica carregada,
  pronta para o Play. Delete remove a faixa selecionada, e "Limpar" esvazia a fila.

Enquanto o player segura o device, **os outros apps ficam sem som nele**, inclusive com a
música pausada. Se houver outra saída, o macOS manda o som deles para ela; num MacBook com fone,
saem pelas caixas. A pausa mantém o device para retomar na hora, sem risco de outro app
tomá-lo. Fechar o app devolve tudo.

## Linha de comando

O mesmo core funciona sem janela:

```
make
./rust/target/release/hog-audio musicas/faixa.flac
./rust/target/release/hog-audio --info musicas/faixa.flac
```

O `--info` mostra o que seria negociado, sem tocar no device:

```
$ ./rust/target/release/hog-audio --info "02. Sugar (feat. Francesco Yates).flac"
fonte    : flac 96000 Hz / 24 bits / 2 canais / 219.1 s
device   : Alto-falantes (MacBook Air)
rate atual: 44100 Hz
rates    : 44100, 48000, 88200, 96000
formatos : 4 físicos disponíveis
           96000 Hz / 32 bits float / 2 canais
           88200 Hz / 32 bits float / 2 canais
           48000 Hz / 32 bits float / 2 canais
           44100 Hz / 32 bits float / 2 canais
negociado: 96000 Hz / 32 bits float
afinação : desligada — saída bit-perfect
```

Outras opções:
- `--volume 35` ou `--volume -18dB`: o volume da reprodução, em porcentagem ou em atenuação exata;
- `--max-volume 60`: muda o teto de segurança (padrão de 50%);
- `--tuning curva.txt`: liga a afinação (ver abaixo);
- `--dump arquivo.raw`: grava os bytes que iriam ao DAC, sem tocar.

## Volume e segurança

Volume no máximo pode machucar o ouvido e danificar o equipamento ligado à saída, seja fone,
caixa ou amplificador; a saída de fone do Mac, por exemplo, entrega tensão suficiente para isso.
Por isso o volume é ajustado **depois de tomar o device e antes de qualquer som sair**, e
devolvido ao valor anterior no fim. Sem `--volume`, ele só é mexido se estiver acima do teto.

Os decibéis vêm do amplificador, não de uma fórmula: pedir `-30dB` dá exatamente −30,0 dB no
device. O número exibido é sempre lido do hardware, porque a conversão que o HAL publica não
bate com a curva que ele aplica.

Duas armadilhas do Core Audio que o código contorna:
- **A primeira escrita de volume depois de reconfigurar o formato é descartada em silêncio.** O
  HAL devolve `noErr`, e a leitura seguinte devolve o valor pedido, vindo de cache. O player
  confere com uma releitura atrasada; um pedido explícito que não se confirma aborta a
  reprodução, em vez de tocar no volume errado.
- **Com o device tomado, o macOS aponta a "saída padrão" para outro device.** Quem lê "o padrão"
  durante a reprodução, inclusive o `system_profiler`, vê o volume do device errado. O player
  nunca confia na saída padrão enquanto segura um device.

## Afinação (`--tuning`)

Um segundo modo de reprodução, que aplica uma curva de resposta em frequência escolhida por você.
**Ligado, a saída deixa de ser bit-perfect**, porque aplicar um filtro é mudar cada amostra. O
hog mode e o rate travado continuam valendo.

A curva é um arquivo de texto com uma frequência e um ganho por linha. CSV de medição funciona
direto, e `#` comenta:

```
# frequência (Hz)   ganho (dB)
20      12.5
1000    -0.6
3200    10.0
20000  -10.6
```

- O filtro é um FIR de fase mínima de 4096 taps, desenhado pelo cepstro real da curva. A
  resposta realizada fica a menos de 0,02 dB da pedida em toda a banda audível.
- O preamp não é opcional: a curva inteira desce até o pico encostar em 0 dBFS, porque ganho
  positivo em digital satura.
- Num device inteiro, a volta de float usa dither TPDF.
- O modo bit-perfect não passa por nenhuma linha da afinação
  ([ADR 0001 do core](rust/docs/adr/0001-modo-afinado-ao-lado-do-bit-perfect.md)).

## Como o bit-perfect é provado

É o que diferencia este projeto de um player comum: qualquer mudança no caminho do áudio tem de
continuar passando nestas provas.

```
make verify FILE=testdata/t96_24.flac BITS=24
make verify-pause FILE=testdata/t96_24.flac
make verify-volume FILE=testdata/t96_24.flac
make verify-lossy FILE=testdata/t44_aac.m4a
```

- **`verify`:** compara amostra por amostra os bytes que o player mandaria ao DAC com o PCM que o
  ffmpeg extrai do mesmo arquivo. O ffmpeg é o oráculo, uma implementação independente. Vale
  para arquivo estéreo em rate que o device suporte.
- **`verify-pause`:** o fluxo com uma pausa no meio sai idêntico ao fluxo sem pausa; nenhum byte
  é perdido nem repetido.
- **`verify-volume`:** as amostras gravadas são idênticas com o volume a 20% e a 90%; o volume
  muda no device, nunca no sinal.
- **`verify-lossy`:** no AAC e no MP3, o dump bate byte a byte com o mesmo arquivo decodificado
  pelo `afconvert`, que usa o mesmo decodificador da Apple. Prova que o app não acrescenta nada
  depois do decodificador. Vale para arquivo estéreo.

O limite delas: todas rodam pelo modo `--dump`, que passa pelo decodificador, pelo ring buffer e
pelo alinhamento de frame, mas não executa o callback de tempo real. Por isso o `io_proc` tem
testes próprios em `rust/src/playback.rs`, chamado direto com um `AudioBufferList` sintético.

As fixtures de teste não são versionadas: `make fixtures` as gera com o ffmpeg e o `afconvert`.

## Proteção contra ruído

Se o formato que o decodificador produz não for exatamente o que o DAC espera, o resultado não é
uma distorção leve: é ruído branco em volume total. Três barreiras impedem isso:
1. O tamanho do frame vem do próprio device, nunca de `bitsPerChannel / 8`: amostras de 24 bits
   podem morar em containers de 32.
2. Depois de configurar a entrega, o formato que o decodificador de fato assumiu é relido e
   comparado com o do device. Se divergir, o player para antes de tocar.
3. No callback, todo byte enviado é dado do arquivo ou zero. Buffer curto, canal excedente ou
   pedido maior que o previsto viram silêncio, nunca memória não inicializada.

## Por que float32 continua sendo bit-perfect

As saídas internas do Apple Silicon só publicam formatos float32. Isso não compromete a
integridade: float32 tem 24 bits de mantissa, então todo inteiro de até 24 bits é representado
exatamente, e a ida e a volta devolvem o mesmo valor. Arquivos acima de 24 bits são recusados
num device float32. Num AAC ou MP3, o decodificador já entrega float32, que vai ao device sem
conversão.

## Se o processo morrer

O modo exclusivo pertence ao PID, e o `coreaudiod` o devolve quando o processo termina, mesmo
com `kill -9`. O Mac nunca fica mudo:
- **Fim da faixa, Ctrl+C, `SIGTERM`, `SIGHUP` ou `SIGQUIT`:** o player devolve o modo exclusivo
  e restaura o rate.
- **`SIGKILL` ou crash:** o sistema devolve o modo exclusivo, mas o rate fica no último usado.
  Tocar qualquer outra coisa ou abrir o Configuração de Áudio e MIDI corrige.

## Limites conhecidos

- **Sem resample:** rate que o device não suporta é recusado, por escolha.
- **Sem gapless:** cada troca de faixa solta e retoma o device, e pode trocar o rate do DAC.
- **Fila básica:** sem seek, aleatório, repetição ou reordenação.
- **Troca de saída:** plugar ou tirar o fone no meio da faixa não migra o áudio; tirar o fone
  para a reprodução e avisa na tela.
- **WAV, AIFF e CAF float de 32 bits** são recusados por engano num device float32 (ver o
  [ROADMAP](docs/ROADMAP.md)).
- **Fone de alta impedância:** a saída que ajusta a tensão existe no MacBook Pro 14"/16" (2021) em
  diante, no MacBook Air M2 e no Mac Studio. O MacBook Air M1 não a tem.

O que vem a seguir está no [ROADMAP](docs/ROADMAP.md).

## Estrutura

Duas linguagens, cada uma com um papel ([ADR 0002](docs/adr/0002-consolidar-o-core-em-rust.md)):

```
rust/      o core inteiro: HAL, formato, ring buffer, IOProc, decodificação e a CLI
apps/      o app SwiftUI (apps/player), que fala com o core por uniffi, e um protótipo em GPUI
tools/     verificador de bit-perfect, gerador de fixtures, ícone e instalação
logos/     a logo e o ícone em SVG
docs/      ROADMAP, ADRs e o registro dos trabalhos
```

O core, por módulo:

```
rust/src/format.rs       decide rate e formato; puro, sem Core Audio
rust/src/volume.rs       interpreta o volume pedido e o teto; puro
rust/src/transitions.rs  regras da máquina de estados; puras
rust/src/ring.rs         fila sem locks entre o decodificador e a thread de tempo real
rust/src/source.rs       decodificação pelo ExtAudioFile
rust/src/device.rs       HAL: modo exclusivo, lock de formato, volume, IOProc, restauração
rust/src/playback.rs     o estado que o IOProc lê e o próprio callback de tempo real
rust/src/engine.rs       a casca imperativa: hardware, arquivo e threads
rust/src/api.rs          a superfície uniffi que o Swift enxerga, só controle
rust/src/main.rs         a CLI
```

A decisão de "isto pode tocar sem perda?" fica isolada do hardware, em código puro: é a regra que
mais importa e a única que dá para verificar sem plugar um fone.

## Desenvolvimento

Num clone limpo, gere primeiro os arquivos de teste; o áudio não entra no repositório:

```
make fixtures    # gera testdata/ com ffmpeg e afconvert
make test        # fmt, clippy e os testes do core e do app
make hw-test     # os testes que tomam o device de verdade, em série (tocam tons)
make app         # monta apps/player/HogAudio.app sem instalar
```

O `make test` usa `clippy -D warnings` como gate, não como sugestão: o `unsafe` do HAL e do
IOProc é onde moram os defeitos caros deste projeto. Teste que depende de fixture e não a acha
reprova, dizendo qual comando rodar.

O IOProc roda em thread de tempo real: dentro dele só existem cópia de memória e operações
atômicas, nada de alocar, travar ou imprimir. Quem mexer nesse caminho começa pelo comentário no
topo de `rust/src/playback.rs`.

## Licença

[MIT](LICENSE). O código de terceiros mantém a própria licença: `apps/gpui/vendor/gpui_macos`
(Apache-2.0, do Zed) e os trechos do protótipo adaptados do Pawse (MIT, com o aviso no próprio
arquivo).
