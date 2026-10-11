# 2. Arquivo com perda toca, sem perda acrescentada pelo app

Data: 2026-10-11

## Status

Aceito. Substitui a regra do README "decide se existe caminho sem perda; se não existir, recusa
tocar" (seção "O que ele faz", item 2). O README muda junto com o código.

## Contexto

Até aqui, "sem perda" se referia ao arquivo. O `source.rs` recusa todo arquivo sem profundidade de
bits, antes de olhar o device, e é assim que AAC e MP3 caem: o decodificador deles entrega float,
e o arquivo não tem bit depth. O próprio README diz que o core lê AAC e MP3, o que hoje não é
verdade.

Parte da biblioteca do usuário é AAC-LC em `.m4a`, de 305 a 339 kbps, com pastas nomeadas pela
master ("[24B-48kHz]"). O usuário ouve de dois jeitos: na escuta dedicada, bit-perfect e na maior
qualidade possível, e no dia a dia, sem essa exigência. A perda que ele aceita é a do arquivo;
a do app, não.

## Decisão

**O app nunca altera uma amostra do que o decodificador entrega.** "Sem perda" passa a descrever
o caminho do app, não o arquivo.

- **Arquivo sem perda** (FLAC, ALAC, WAV, AIFF): o decodificador entrega o próprio arquivo, e a
  reprodução continua bit-perfect, pelo mesmo código de hoje.
- **AAC-LC e MP3:** tocam no modo exclusivo, com o float32 do decodificador entregue intacto a um
  formato físico float32 do device, no rate do próprio arquivo. O `negotiate()` só aceita a faixa
  com perda nessa condição.
- **Continua recusado** tudo que obrigaria o app a mexer nas amostras: resample (rate que o device
  não suporta) e arredondar o float do decodificador para inteiro (device sem float32).
- **O modo afinado** ([ADR 0001 do core](0001-modo-afinado-ao-lado-do-bit-perfect.md)) continua
  a única exceção declarada, escolhida pelo usuário e sinalizada.

O `FileFormat` passa a dizer qual dos dois casos é (`Resolution::Lossless { bits }` ou
`Resolution::Lossy { bitrate_kbps }`), e a API pública troca `bit_depth: u32` por
`bit_depth: Option<u32>` mais `bitrate_kbps: Option<u32>`. A tela mostra "AAC 339" no lugar de
"24/48".

O escopo foi fechado no device do usuário, um MacBook Air M1, cujas saídas só publicam float32.
Ali a faixa com perda sempre tem caminho.

## Alternativas descartadas

**Declarar 24 bits para AAC e MP3 no `source.rs`.** É o menor diff, e o `negotiate()` nem muda.
Mas a tela mostraria "24 bit" num MP3. E num device que preferisse inteiro, o `is_better`
escolheria int24, e o `ExtAudioFile` arredondaria o float sem ninguém ver. A garantia passaria a
depender do device, em silêncio.

**Converter para inteiro com dither TPDF**, reaproveitando o quantizador da afinação. Abre os DACs
que só aceitam inteiro, mas é perda acrescentada pelo app. Fica para quando houver um DAC assim.

**Um caminho de produção separado para a faixa com perda**, como o da afinação. Não há o que
processar: a entrega já é float32, e o `produce_direct` serve igual.

**Usar o ffmpeg como oráculo da prova.** O decodificador de AAC e MP3 dele difere do da Apple nos
últimos bits. A prova da faixa com perda compara o dump com o `afconvert`, que usa o mesmo
decodificador por outra ferramenta.

## Consequências

- O caminho sem perda não muda: `device.rs`, `playback.rs`, `ring.rs`, o IOProc e a ordem hog →
  rate → formato → volume ficam intactos, e as provas `make verify*` continuam valendo.
- Entra uma prova nova, `make verify-lossy`, que compara o dump do player com o `afconvert` em
  float32.
- A mensagem de recusa nomeia o codec ("aach", não "1633772320"): o `four_cc` corta o espaço
  final e o ponto inicial.
- Ficam de fora, registrados no `docs/ROADMAP.md`, Fase 0.1:
  - o resample;
  - o HE-AAC, o Opus e os outros codecs com perda;
  - o WAV float de 32 bits, hoje recusado por engano num device float32;
  - os DACs que só aceitam inteiro;
  - o AAC com DRM e o arquivo corrompido.
- AAC e MP3 em 22,05 ou 32 kHz continuam recusados neste Mac, pela mesma regra do FLAC de
  192 kHz: rate não suportado.

Desenho e provas em `docs/superpowers/specs/2026-10-11-audio-com-perda-design.md`.
