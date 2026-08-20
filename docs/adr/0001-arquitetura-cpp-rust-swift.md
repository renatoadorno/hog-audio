# 1. Arquitetura em hub: C++ no core, Rust nas integrações, Swift na interface

Data: 2026-08-16

## Status

**Superseded pelo [ADR 0002](0002-consolidar-o-core-em-rust.md)** em 19/08/2026. O core não é
mais C++: o port para Rust foi feito, provado bit-perfect contra o `ffmpeg`, e a interface
gráfica foi construída sobre ele. O `cpp/` foi removido do repositório.

Este documento fica pelo registro do raciocínio — em especial a regra de tempo real do IOProc,
que continua valendo, e a alternativa "reescrever tudo em Rust", que ele deixou aberta e que o
0002 exerceu.

## Contexto

O `hog-audio` hoje é uma CLI em C++ que reproduz um arquivo local pelo caminho digital mais
curto que o macOS permite: modo exclusivo (hog mode), sample rate travado no do arquivo e
`AudioDeviceIOProc` alimentado diretamente, sem passar pelo mixer.

O objetivo declarado do projeto evoluiu: **caminho limpo a partir de qualquer fonte**, não
apenas de arquivos locais. Um FLAC de 96/24 tira muito mais proveito disso do que um stream
lossy, mas a proposta é que funcione com o que estiver disponível — implementado aos poucos,
porque cada formato tem particularidades que exigem teste próprio.

Duas necessidades novas decorrem disso:

1. **Integração com Spotify.** O `librespot` é a única implementação aberta do protocolo
   Spotify Connect, e é escrita em Rust. Isso torna o Rust obrigatório em algum ponto.
2. **Interface gráfica nativa.** Um player de alta fidelidade tem a interface como vitrine, e
   o alvo é exclusivamente macOS. SwiftUI é a escolha natural, e já é território conhecido
   (projeto `atrium`).

A pergunta que este ADR responde: **como essas três linguagens se organizam entre si?**

### Fatos verificados antes de decidir

Cada um destes foi confirmado, não presumido:

- **Rust faz hog mode nativamente.** Teste executado nesta máquina com `coreaudio-sys 0.2.18`
  obteve exclusividade do device (dono = PID do processo), travou o rate de 48000 para 44100 Hz
  e restaurou tudo. Os 10 símbolos necessários existem nos bindings, inclusive
  `ExtAudioFileOpenURL`. Atenção: o crate de alto nível `coreaudio-rs` **não** expõe hog mode
  (cobre AudioUnit); quem resolve é o `coreaudio-sys`.
- **Swift chama C++ diretamente** desde a 5.9, com melhorias relevantes na 6.2 — antes era
  necessário criar wrappers em C, agora não. O Swift não precisa do Rust para falar com o core.
- **`uniffi-rs` é adequado para controle, não para streaming.** Produção-quality para Swift
  (usado no Firefox), suporta `Vec<u8>`, `&[u8]`, records, enums e callbacks. Mas é orientado a
  API de aplicação: a documentação não trata de zero-copy, chamadas de alta frequência nem
  tempo real.
- **Rust↔C++ não é FFI direto.** Rust fala C, não C++. A ponte exige fachada `extern "C"` ou o
  crate `cxx`, que declara overhead zero ou negligenciável — baixo, mas não automático.
- **O Spotify não entrega lossless por Connect.** O tier lossless (24-bit/44.1 kHz FLAC, set/2025)
  funciona só nos apps oficiais. Endpoints Connect, incluindo librespot, recebem Ogg Vorbis
  320 kbps. Não é DRM: o backend simplesmente não serve FLAC a terceiros
  ([librespot#1583](https://github.com/librespot-org/librespot/issues/1583)). Portanto o caminho
  Spotify será sempre "caminho limpo a partir de fonte lossy" enquanto isso não mudar — o que
  ainda evita resample e mixer, mas não é bit-perfect do master.

## Decisão

Adotar uma topologia de **hub**, com o Swift orquestrando, e **não** uma escada
`Swift → Rust → C++`.

```
Swift (UI/UX)
  ├── C++ core     ← controle: play, device, volume, formato     [interop Swift/C++ direto]
  └── Rust         ← Spotify: login, busca, playlists, metadados [uniffi]

Rust (librespot) ──[ring buffer compartilhado]──> C++ core ──> DAC
```

O Rust **não** é camada intermediária: é mais uma fonte de áudio plugada no core, ao lado do
`ExtAudioFile` já existente, e o provedor das integrações de rede.

### Papel de cada linguagem

| Camada | Responsabilidade | Por quê |
|---|---|---|
| C++ | HAL, hog mode, lock de formato, IOProc, ring buffer, negociação | Já existe, testado, e resolveu armadilhas caras |
| Rust | librespot e integrações futuras de rede | Única implementação aberta do Connect |
| Swift | UI, orquestração, integração com o sistema | Nativo no alvo; visual e Now Playing de graça |

### Natureza de cada fronteira

As fronteiras têm naturezas diferentes e por isso ferramentas diferentes:

| Fronteira | Natureza | Ferramenta |
|---|---|---|
| Swift → C++ | controle, baixa frequência | interop nativo do Swift 6 |
| Swift → Rust | controle, metadados, rede | `uniffi` |
| Rust → C++ (áudio) | fluxo contínuo, tempo real | ring buffer cru + ponteiro |
| Rust → C++ (controle) | raro | `cxx` ou `extern "C"` |

### Regra de design derivada

**O IOProc nunca chama Rust nem Swift.** Ele apenas lê do ring buffer. Toda travessia de
fronteira fica do lado do produtor, onde bloquear é aceitável. Essa é a mesma disciplina de
tempo real que o core C++ já segue — e vale reforçá-la porque em Rust é igualmente fácil alocar
sem perceber dentro de um callback.

## Alternativas consideradas

**Escada estrita `Swift → Rust → C++`.** Descartada por dois motivos. Primeiro, forçaria toda
chamada de controle a atravessar o Rust, inclusive tocar um arquivo local — indireção no caminho
mais comum e mais crítico do app, para servir a um caso que é minoria do uso. Segundo, o áudio
do librespot é fluxo contínuo, não chamada de API: mandá-lo por `uniffi` seria usar a ferramenta
errada.

**JUCE Framework.** Descartado. Sua principal vantagem é portabilidade, e este projeto é
intrinsecamente macOS — Core Audio HAL, hog mode e `PhysicalFormat` não têm equivalente
portável. Além disso, o JUCE abstrai exatamente a camada que dá valor ao projeto, desenha a
própria GUI em vez de usar a do sistema, e seus casos fortes (DSP, hospedar plugins)
contradizem a premissa: qualquer EQ ou resample na cadeia encerra o bit-perfect por definição.

**Reescrever tudo em Rust (duas linguagens: Rust + Swift).** Tecnicamente viável — o teste
provou que o Rust faz o HAL — e seria mais simples de construir e manter. Descartada por
preservação de risco: o core C++ já resolveu problemas que ninguém acerta de primeira (escrita
de volume descartada em silêncio, desalinhamento de frame no underrun, ordem hog→rate→formato).
Essa experiência vale mais que o código. Fica registrado que esta alternativa continua aberta:
são ~1.200 linhas, das quais ~400 são núcleo puro cujos testes já servem de especificação.

## Consequências

### Positivas

- Cada linguagem atua onde é mais forte, sem lutar contra o ecossistema.
- O core testado é preservado, com seu histórico de correções.
- As fronteiras ficam explícitas e cada uma pode ser validada isolada.
- A UI é nativa: visual do sistema, teclas de mídia e Now Playing sem esforço extra.
- O Rust abre porta para outras integrações além do Spotify.

### Negativas, aceitas conscientemente

- **Três toolchains** (CMake, Cargo, SPM/Xcode) precisam produzir um app assinável. Este é o
  custo real do desenho, e é maior que o da ponte Swift↔Rust — que o `uniffi` torna a fronteira
  mais fácil das três, ao contrário da suposição inicial.
- **Uma fronteira a mais** do que a alternativa de duas linguagens, em troca de não reescrever
  código testado.
- **Depuração atravessa três linguagens**: um crash no IOProc alimentado por Rust e disparado
  pelo Swift é consideravelmente mais caro de investigar.
- **O caminho Spotify não será bit-perfect do master** enquanto o Connect não servir FLAC. O
  ganho ali se limita a evitar resample e mixer — real, mas menor que com arquivos locais.

## Ordem de implementação

Cada fronteira é validada isolada, para que um custo inesperado apareça antes de tudo estar
amarrado:

1. **Swift ↔ C++ direto** — prova a UI contra o core que já funciona, sem Rust nenhum.
2. **Rust como fonte de áudio** — o Spotify entra sem tocar na UI já validada.
3. **Demais integrações** em Rust, conforme a necessidade aparecer.

## Referências

- [Mixing Swift and C++ — Swift.org](https://www.swift.org/documentation/cxx-interop/)
- [Constraints of C++ Interoperability — Swift.org](https://www.swift.org/documentation/cxx-interop/status/)
- [UniFFI — Swift Bindings](https://mozilla.github.io/uniffi-rs/latest/swift/overview.html)
- [CXX — safe interop between Rust and C++](https://cxx.rs/)
- [coreaudio-sys](https://crates.io/crates/coreaudio-sys)
- [librespot#1583 — Spotify lossless will not be supported](https://github.com/librespot-org/librespot/issues/1583)
