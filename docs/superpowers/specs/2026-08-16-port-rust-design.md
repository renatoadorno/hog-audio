# Port do hog-audio para Rust com prova de bit-perfect

Data: 2026-08-16

## Objetivo

Reimplementar em Rust tudo que o `hog-audio` faz hoje em C++, e **provar objetivamente** que
a versão Rust preserva a propriedade bit-perfect. O resultado é um dado para decidir como o
projeto evolui — não uma migração já decidida.

A dúvida específica que motiva o experimento: o HAL do Core Audio é a parte que menos se
beneficia do Rust (será quase toda `unsafe`), e é justamente a parte mais crítica. O
experimento mede se isso é um problema real ou uma preocupação teórica.

## Contexto

O código atual são 2.076 linhas: 1.486 de implementação e 590 de teste, distribuídas em
núcleo puro (negociação de formato, ring buffer, volume), camada HAL (hog mode, lock de
rate/formato, IOProc, restauração RAII) e CLI.

Já foi verificado empiricamente que Rust faz hog mode nativamente via `coreaudio-sys`: um
teste executou nesta máquina, obteve exclusividade do device, travou o rate de 48000 para
44100 Hz e restaurou tudo. Ver [ADR 0001](../../adr/0001-arquitetura-cpp-rust-swift.md).

## Critério de sucesso

O experimento produz uma resposta binária mais uma comparação de esforço.

**A prova de bit-perfect** usa o ffmpeg como oráculo externo:

```
ffmpeg -i t96_24.flac -f s24le ref.raw       # int24 puro extraído do FLAC
cpp/hog-audio  --dump cpp.raw  t96_24.flac    # bytes que iriam ao IOProc
rust/hog-audio --dump rust.raw t96_24.flac

tools/verify_bitperfect.py ref.raw cpp.raw    # converte float32→int24, compara
tools/verify_bitperfect.py ref.raw rust.raw
cmp cpp.raw rust.raw
```

As três comparações precisam bater. Isso prova simultaneamente que o C++ é bit-perfect contra
referência externa, que o Rust também é, e que a afirmação `int24 → float32 → int24 é exato`
— hoje documentada mas não testada — é verdadeira.

**A comparação de esforço** registra: linhas de código por módulo, atritos encontrados, e
quanto `unsafe` a camada HAL exigiu.

## Requisitos

### O modo `--dump`

Precisa percorrer **o mesmo pipeline** da reprodução, trocando apenas o consumidor final:

```
decodificador → ring buffer → consumidor → DAC        (normal)
decodificador → ring buffer → consumidor → arquivo    (--dump)
```

O consumidor do dump simula o IOProc: pede blocos de 512 frames em laço, usando a mesma
lógica de `alignedReadSize`, e grava o resultado. Um atalho que apenas decodificasse e
gravasse pularia ring buffer, alinhamento de frame e desintercalação — exatamente onde
estiveram os bugs mais caros do projeto.

O `--dump` **toma o device** (hog + lock de formato) sem chamar `start`. É a única forma de
ler o `streamFormat` real em vez de presumi-lo. Custa silenciar o sistema por 1-2 segundos.

O modo é implementado nas **duas** versões, C++ e Rust, com comportamento idêntico.

### Paridade funcional

Tudo que o C++ faz: negociação de formato com recusa de resample, hog mode, lock de nominal
rate e formato físico/virtual, decodificação via `ExtendedAudioFile`, ring buffer SPSC com
alinhamento de frame, controle de volume com teto de segurança e escrita confirmada,
restauração completa via RAII, tratamento de SIGINT/SIGTERM/SIGHUP/SIGQUIT, e a mesma CLI.

### Paridade de testes

Os mesmos casos de teste, em `#[test]` nativo do Rust: 45 checks de negociação de formato, 42
de ring buffer, 40 de volume. Eles são a especificação executável do port.

### Restrições deliberadas

- **Dependência única: `coreaudio-sys`.** Sem `clap`, sem `rtrb`, sem nada. O C++ não tem
  dependência externa; a comparação exige o mesmo.
- **O ring buffer é portado, não substituído.** `rtrb` seria a escolha idiomática em produção,
  mas o alinhamento de frame é lógica própria e os mesmos testes precisam valer nos dois lados.
- **Parsing de CLI manual**, como no C++.

## Estrutura

```
hog-audio/
├── cpp/          src/, tests/, CMakeLists.txt   (movidos; histórico git preservado)
├── rust/         Cargo.toml, src/, tests/
├── tools/        verify_bitperfect.py
├── Makefile      orquestra ambos e a comparação
└── docs/, musicas/, testdata/
```

### Mapa do port

| C++ | Rust | Natureza |
|---|---|---|
| `format_negotiation.{hpp,cpp}` | `src/format.rs` | puro |
| `volume.{hpp,cpp}` | `src/volume.rs` | puro |
| `ring_buffer.hpp` | `src/ring.rs` | puro |
| `audio_source.{hpp,cpp}` | `src/source.rs` | FFI `ExtendedAudioFile` |
| `hog_device.{hpp,cpp}` | `src/device.rs` | FFI HAL |
| `main.cpp` | `src/main.rs` | CLI |

## Riscos conhecidos

- **O IOProc em Rust é `extern "C"` em thread de tempo real.** Rust não impede alocação ali:
  um `format!`, um `Vec` que cresce ou um `println!` quebram o áudio em silêncio. Exige a
  mesma disciplina aplicada ao C++.
- **A camada HAL será quase toda `unsafe`**, então as garantias do Rust ajudam pouco
  exatamente onde o risco é maior. Medir isso é parte do objetivo.
- **As armadilhas do Core Audio se repetem**: escrita de volume descartada em silêncio após
  reconfiguração, mudança de rate assíncrona, desalinhamento de frame no underrun. O C++
  atual serve de especificação; se o Rust exigir mais código para os mesmos contornos, isso é
  resultado do experimento, não defeito.

## Fora de escopo

Integração com Spotify/librespot, interface gráfica, e qualquer decisão sobre migrar ou não.
Este documento cobre apenas o experimento que informa essa decisão.
