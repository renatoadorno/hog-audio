# Áudio com perda (AAC e MP3): design

Data: 2026-10-11
Estado: aprovado em conversa, parte por parte; falta a revisão desta spec escrita.
Origem: Fase 0.1 do `docs/ROADMAP.md`. Decisão registrada na
[ADR 0002 do core](../../../rust/docs/adr/0002-arquivo-com-perda-sem-perda-do-app.md).

## Objetivo

Tocar AAC-LC e MP3 no modo exclusivo sem que o app acrescente perda. A perda permitida é só a do
arquivo.

O usuário ouve de dois jeitos: na escuta dedicada, bit-perfect e na maior qualidade possível, e
no dia a dia, sem essa exigência. Parte da biblioteca dele é AAC-LC em `.m4a`, de 305 a 339 kbps,
e hoje o core recusa esses arquivos.

## Requisito

**O app nunca altera uma amostra do que o decodificador entrega.**

- Num arquivo sem perda (FLAC, ALAC, WAV, AIFF), o que o decodificador entrega é o próprio
  arquivo. A reprodução continua bit-perfect, como hoje.
- Num AAC-LC ou MP3, o decodificador entrega float32. Esse float32 vai intacto ao device float32,
  no rate do próprio arquivo, sem resample e sem arredondamento.

## Escopo

Entra:
- AAC-LC (codec `'aac '`) em qualquer contêiner que o `ExtAudioFile` abra, como `.m4a`;
- MP3 (codec `'.mp3'`);
- só o device do usuário: o MacBook Air M1, cujo fone e caixas publicam apenas formatos físicos
  float32. Outros DACs ficam desconsiderados nesta rodada.

Fica para depois: a seção "O que fica para depois", no fim.

## Restrições

- **O caminho sem perda não muda.** O FLAC e os outros formatos sem perda seguem o mesmo código e
  as mesmas provas (`make verify*`).
- **`device.rs`, `playback.rs`, `ring.rs`, o IOProc e a ordem hog → rate → formato → volume não
  mudam.** O diff em `rust/` mostra isso.
- **O rate que o device não suporta continua recusado**, para qualquer formato.
- **Tudo que toma o device é avisado antes**, com o volume em ~10% antes de qualquer som.
- **Commits** no padrão do commit-guard; sem push nem merge sem pedido.

## Desenho

### O modelo e o core

- **`rust/src/source.rs`**
  - O `FileFormat` troca o `bit_depth: u32` por uma resolução explícita:
    - `Resolution::Lossless { bits: u32 }`;
    - `Resolution::Lossy { bitrate_kbps: Option<u32> }`.
  - A classificação, em ordem:
    1. Bit depth conhecido, pelo `mBitsPerChannel` ou pelas flags do ALAC: `Lossless`, como hoje.
    2. Sem bit depth e codec `'aac '` ou `'.mp3'`: `Lossy`. O bitrate vem do `AudioFile` por
       baixo do `ExtAudioFile` (`kExtAudioFileProperty_AudioFile` e depois
       `kAudioFilePropertyBitRate`), arredondado para kbps. Se a leitura falhar, fica `None`, e a
       faixa toca mesmo assim.
    3. Qualquer outro codec sem bit depth é recusado na abertura (ver "Erros e casos de borda").
  - O `four_cc` corta o espaço final e o ponto inicial: `'aac '` vira "aac" e `'.mp3'` vira
    "mp3". Hoje `'aac '` cai no decimal ("1633772320"), porque o espaço não é caractere gráfico.
- **`rust/src/format.rs`, no `negotiate()`**
  - `Lossless`: a regra de hoje, sem mudança.
  - `Lossy`: só serve um formato físico float32 (`SampleType::Float`, 32 bits) que cubra o rate
    do arquivo. É aqui que mora a regra "o app não altera amostras". Num device sem float32 a faixa
    é recusada; neste Mac isso não acontece.
  - O rate não suportado é recusado antes, com a mensagem de hoje, para os dois casos.
- **`rust/src/engine.rs`**
  - A lógica da partida não muda. O `start_common()` já entrega no formato do device
    (`client_format_for`), então o `ExtAudioFile` repassa a saída do decodificador em float32
    pelo mesmo `produce_direct` do FLAC.
  - O `LoadedTrack` troca `bit_depth: u32` pelos mesmos campos da API: `bit_depth: Option<u32>`
    e `bitrate_kbps: Option<u32>`.
- **Não mudam:** `device.rs`, `playback.rs`, `ring.rs`, `recovery.rs`, `device_watch.rs` e o
  modo afinado. A afinação já pede float32 ao decodificador e funciona com AAC e MP3 sem mudança.

### A API e as telas

- **`rust/src/api.rs`**
  - O `TrackFormat` e o `TrackProbe` trocam `bit_depth: u32` por `bit_depth: Option<u32>`, vazio
    na faixa com perda, e ganham `bitrate_kbps: Option<u32>`, preenchido só na faixa com perda.
  - O `codec` chega limpo: "aac", "mp3", "flac", "alac".
- **Swift (`apps/player`)**
  - `Quality.swift`: as duas funções de formatação recebem os campos opcionais.
    - Sem perda, nada muda: "44.1 kHz · 24 bit · flac" na linha técnica e "24/44.1" no selo.
    - Com perda: "48 kHz · AAC 339 kbps" na linha técnica e "AAC 339" no selo da fila.
    - Com perda e sem bitrate: "48 kHz · AAC" e "AAC".
    - A linha técnica e o tooltip do selo continuam saindo da mesma função.
  - `DisplayState.swift` e `QueueEntry.swift` passam os campos novos. O "hog ativo" continua.
  - Os testes que montam `TrackFormat` e `TrackProbe` se ajustam. As bindings
    (`HogAudioBindings/hog_audio.swift`) são regeneradas pelo `make`.
- **GPUI (`apps/gpui`)**
  - A mesma regra de exibição em `model/src/format.rs`, `entry.rs` e `display.rs`, só para o
    protótipo compilar e o `make gpui-test` ficar verde.
- **CLI (`rust/src/main.rs`)**
  - A linha "arquivo" de uma faixa com perda vira `aac — 48000 Hz / 339 kbps / 2 canais`. A de
    uma faixa sem perda não muda.
- **Abrir arquivos** não muda: o app já aceita qualquer áudio (`AudioFiles.swift`). Um codec que
  o core não toca entra na fila com o selo de problema e o motivo.

O visual final dos selos é do protótipo da interface. Esta rodada garante só que nenhuma tela
mostra "0 bit".

### Erros e casos de borda

- **Codec com perda fora do escopo** (HE-AAC `aach` e `aacp`, Opus e outros): recusado na
  abertura com uma mensagem que diz o porquê, por exemplo "aach: sem profundidade de bits e fora
  dos codecs com perda suportados (AAC-LC e MP3)". O caminho é o de hoje: falha da faixa, selo de
  problema na fila, auto-avanço pula.
- **AAC ou MP3 em rate que o device não suporta** (22,05 ou 32 kHz neste Mac): recusado com a
  mensagem atual, "Reproduzir exigiria resample".
- **Device sem float32 no rate do arquivo:** recusado com "entregar em inteiro arredondaria o que
  o decodificador produziu".
- **Bitrate ilegível:** toca, e a tela mostra só o codec.
- **MP3 mono:** vai aos dois canais, como o FLAC mono já vai. Nenhuma amostra muda de valor.
- **MP3 VBR sem cabeçalho Xing:** o total de frames pode ser estimado errado, e o relógio mostraria
  um total diferente do real. O fim da faixa vem da produtora esgotando o arquivo, e a barra já
  é limitada a 100%. Aceito.
- **Atraso do encoder do AAC (priming):** o `ExtAudioFile` descarta o silêncio inicial pelos
  metadados do `.m4a`.
- **AAC com DRM (`.m4p`) ou arquivo corrompido:** continua o comportamento de hoje. Erro de
  decodificação no meio devolve zero frames, e a faixa termina ali. Limite conhecido.
- **Recuperação de device perdido, `device_watch` e troca de rate entre faixas:** não mudam. A
  faixa com perda passa pelo mesmo hog e pelo mesmo IOProc.

### Provas

As do caminho sem perda continuam e precisam ficar verdes:
- `make verify FILE=testdata/t96_24.flac BITS=24` e `make verify FILE=testdata/t44_16.flac BITS=16`;
- `make verify-pause` e `make verify-volume`;
- `make hw-test`, com o app instalado fechado;
- `make test` (fmt, clippy, Rust e Swift) e `make gpui-test`.

Prova nova, `make verify-lossy FILE=...`:
- O dump do player (os bytes que o IOProc copiaria) é comparado byte a byte com o mesmo arquivo
  decodificado pelo `afconvert` em float32 little-endian, no rate do arquivo.
- O `afconvert` usa o mesmo decodificador da Apple por outra ferramenta. Bater byte a byte prova
  que o app não acrescentou nada depois do decodificador.
- O ffmpeg não serve de oráculo: o decodificador de AAC e MP3 dele difere do da Apple nos últimos
  bits, e a comparação falharia pelo motivo errado.
- Se o `afconvert` divergir (por exemplo, no tratamento do priming), a comparação passa a ser
  contra uma decodificação direta do `ExtAudioFile` em teste. É uma prova mais fraca, e o README
  diz isso.

Testes novos:
- `format.rs`:
  - com perda em device float32 é aceita;
  - com perda em device só inteiro é recusada;
  - com perda em rate não suportado é recusada;
  - com perda escolhe float mesmo quando há formato inteiro no rate.
- `source.rs`:
  - o `four_cc` limpo de `'aac '` e `'.mp3'`;
  - AAC-LC e MP3 abrem como `Lossy`, com bitrate;
  - HE-AAC é recusado com a mensagem nova.
- `api.rs`: o `probe_track` de um AAC devolve `bit_depth` vazio e o bitrate.
- Swift e GPUI: "AAC 339", "48 kHz · AAC 339 kbps" e o caso sem bitrate.
- `tools/make_fixtures.sh` gera um AAC-LC, um MP3 e um HE-AAC (pelo `afconvert`, só para provar a
  recusa). Não dependem da PR #1 (`fix/fixtures-capa-m4a-mp3`).

Escuta real: uma faixa AAC da biblioteca ("On My Mind"), com aviso antes e volume em ~10%. O
`verify-lossy` também toma o device sem tocar som, como os outros dumps, e é avisado antes.

### Documentação

- A ADR 0002 do core registra a regra nova e o conflito com a regra antiga do README.
- O README é atualizado junto com o código, não antes, porque descreve o projeto como ele é:
  - os itens 1 e 2 de "O que ele faz";
  - a seção "Por que float32 continua sendo bit-perfect", com a faixa com perda;
  - os limites conhecidos;
  - a prova nova, ao lado das três de hoje.
- O ROADMAP registra o que foi decidido e o que ficou para depois.

## Riscos

- **O `afconvert` divergir do `ExtAudioFile`** no priming ou no tamanho do fim. O plano confere
  isso primeiro; a saída está em "Provas".
- **HE-AAC com sinalização implícita** pode chegar como `'aac '` no rate do núcleo (22,05 ou
  24 kHz). Neste Mac esses rates são recusados. Se um dia forem aceitos, a faixa tocaria sem a
  banda do SBR.
- **Bitrate de VBR** é a média que o `AudioFile` publica, e em MP3 sem cabeçalho pode ser
  estimado.

## O que fica para depois

- **Resample:** uma lógica que resampleia só em casos específicos, a decidir. Até lá, rate não
  suportado continua recusado.
- **HE-AAC (`aach`, `aacp`), Opus e outros codecs com perda:** entram quando um teste provar o
  rate e o formato de entrega de cada um.
- **WAV, AIFF e CAF float de 32 bits:** hoje recusados num device float32, embora float32 para
  float32 seja exato (conferido com `--info` em 11/10). Falso negativo do `preserves_every_bit`,
  que olha só o bit depth.
- **DACs que só aceitam inteiro:** AAC e MP3 são recusados neles. Converter exigiria arredondar,
  com ou sem dither.
- **AAC com DRM e arquivo corrompido:** a faixa termina no erro, sem mensagem.
- **Fase 0.2, o modo exclusivo ligável:** inteira, depois desta.
