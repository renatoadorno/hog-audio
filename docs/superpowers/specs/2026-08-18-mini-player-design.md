# Mini player: engine Rust com interface SwiftUI

Data: 2026-08-18

## Objetivo

Transformar a CLI `hog-audio` num mini player de faixa única, com interface nativa. O player
carrega **um** arquivo por vez — sem lista, sem fila, sem biblioteca —, toca, pausa, controla
volume e mostra os metadados da faixa.

A prioridade declarada é **núcleo funcional e bem testado**, não interface bonita. A UI desta
versão é utilitária e descartável; o que precisa sobreviver é o engine.

A garantia de bit-perfect, provada byte a byte contra o oráculo do ffmpeg, **não pode
regredir**. É pré-requisito, não um objetivo entre outros.

## Contexto

O repositório tem hoje duas implementações completas e equivalentes do player de linha de
comando — `cpp/` e `rust/` —, com paridade comprovada. A decisão registrada é consolidar em
Rust. Esta é a primeira etapa que age sobre essa decisão: o Rust ganha a camada que o C++ não
vai ganhar.

O `rust/` é hoje **binary-only**. O `main.rs` tem 637 linhas e o `run()` faz tudo em sequência
bloqueante: abre o arquivo, consulta o device, negocia formato, sequestra, dispara a thread
produtora, cria o IOProc, espera terminar, restaura. Não existe crate de biblioteca nem máquina
de estados — o player só sabe "tocar até o fim ou até o Ctrl+C".

O ADR 0001 desenhou uma topologia em hub com o C++ no core. Esta spec segue a alternativa que o
próprio ADR deixou registrada como aberta — Rust + Swift, duas linguagens — porque o port
provou que o HAL em Rust não é obstáculo.

### Restrição que decide a topologia sozinha

**Hog mode é por PID.** O engine Rust tem de rodar dentro do processo do app Swift, como
biblioteca linkada. Se o Swift lançasse o binário atual como subprocesso, o hog pertenceria ao
filho e play/pause/volume virariam IPC — complexidade sem contrapartida.

### Decisões tomadas com o usuário

| Decisão | Escolha |
|---|---|
| Pause sob hog | `AudioDeviceStop` mantendo o hog. Resume instantâneo; o Mac fica mudo enquanto o player estiver aberto |
| Fronteira Swift↔Rust | `uniffi` 0.32 em modo proc-macro, sem arquivo `.udl` |
| Build do Swift | Swift Package Manager; o Makefile monta o `.app` |
| Escopo além do obrigatório | Tempo decorrido/duração, indicadores técnicos, capa do álbum |
| Fora de escopo | Seek, playlist, gapless, Now Playing do sistema |

### Fatos verificados antes de decidir

Medidos nesta máquina, contra os arquivos reais do projeto:

- **`uniffi 0.32.0`** é a versão corrente e Swift é linguagem de primeira classe.
- **`AVAsset.commonMetadata` devolve lista vazia para FLAC.** O caminho multi-formato
  conveniente não serve para o formato principal do projeto.
- **`AVAsset.metadata` devolve as tags de FLAC** no keyspace `vorb`: `TITLE`, `ARTIST`,
  `ALBUM`, `ALBUMARTIST`, `DATE`, `TRACKNUMBER`, `GENRE`.
- **A capa chega pronta.** O item `METADATA_BLOCK_PICTURE` entrega em `dataValue` bytes que
  começam em `FF D8 FF E0` — JPEG com marcador JFIF —, e `NSImage(data:)` os aceita direto. O
  AVFoundation já desmontou o cabeçalho do bloco de imagem do FLAC. Verificado em
  `Skyfall.flac`, 38.583 bytes, 600×600.
- Toolchain no lugar: Swift 6.3.1, Xcode 26.4.1, rustc 1.93.1.

Consequência: **metadados são responsabilidade do Swift**. Em Rust exigiriam reimplementar
Vorbis comments e o bloco de imagem do FLAC; em Swift são cerca de vinte linhas sem dependência
alguma. A capa nunca cruza a fronteira uniffi.

## Arquitetura

```
apps/player (Swift)
  └── HogPlayerKit ──[uniffi]──> hog-audio (lib Rust) ──> Core Audio HAL ──> DAC
  └── AVFoundation ──> metadados e capa
```

O Swift orquestra e desenha. O Rust é dono do áudio e do device. Nenhum dado de áudio cruza a
fronteira: o que atravessa são comandos e um snapshot de estado.

### Estrutura de arquivos

```
rust/
  Cargo.toml            [lib] crate-type = ["lib", "staticlib"] + [[bin]] hog-audio
  build.rs              linka CoreFoundation (já existe) + scaffolding uniffi
  src/
    lib.rs              NOVO — raiz da biblioteca, declara os módulos e uniffi::setup_scaffolding!()
    api.rs              NOVO — superfície uniffi: HogPlayer, Snapshot, PlayerState, PlayerError
    engine.rs           NOVO — máquina de estados e orquestração, extraídas do run()
    transitions.rs      NOVO — regras de transição, puras, sem hardware
    main.rs             CLI, agora consumindo engine; mantém --dump
    device.rs           contrato inalterado
    format.rs           intocado
    ring.rs             intocado
    source.rs           contrato inalterado
    volume.rs           intocado
    ffi.rs              intocado

apps/player/
  Package.swift
  Sources/HogPlayer/            app SwiftUI: janela, botões, slider
  Sources/HogPlayerKit/         lógica testável: metadados, formatação, ViewModel
  Sources/HogAudioFFI/          alvo C com o header e o modulemap gerados
  Sources/HogAudioBindings/     Swift gerado pelo uniffi-bindgen
  Tests/HogPlayerKitTests/

cpp/                            congelado, intocado
Makefile                        + alvos rust-lib, bindings, swift-test, app, run-app
tools/verify_bitperfect.py      intocado
```

A CLI continua existindo e continua sendo quem roda `make verify`. Ela é o gate de regressão do
refactor: se o engine extraído mudar o comportamento, a prova de bit-perfect quebra e nós
ficamos sabendo.

### Máquina de estados

```
Idle ──load──> Loaded ──play──> Playing ⇄ Paused
                                   │
                                   └── fim da faixa ──> Finished ──play──> Playing

qualquer estado ── falha assíncrona ──> Failed
```

| Estado | Significado | Device |
|---|---|---|
| `Idle` | Nenhum arquivo carregado | Livre |
| `Loaded` | Arquivo válido e negociado | Livre |
| `Playing` | IOProc alimentando o DAC | Sequestrado |
| `Paused` | IOProc parado | Sequestrado |
| `Finished` | Faixa terminou | Sequestrado |
| `Failed` | Erro com mensagem | Livre |

Erros **síncronos** — arquivo ilegível, rate não suportado, hog negado — não mudam o estado: o
comando devolve `Err` e o Swift mostra a mensagem. O estado `Failed` existe para o que nenhuma
chamada pode devolver, que é a falha assíncrona da thread produtora durante a reprodução; ao
entrar em `Failed` o engine restaura e solta o device, por isso a tabela o marca como livre.

Comandos e o que cada um faz:

- **`load(path)` não toca no device.** Abre o arquivo, lê o formato, consulta as capacidades do
  device — leitura pura, sem hog — e roda `negotiate()`. Um arquivo de 192 kHz num device que
  não suporta falha aqui, com o Mac ainda intacto. Ninguém fica mudo por abrir um arquivo.
  Chamar `load` é permitido em qualquer estado: se o device estiver adquirido, ele é parado e
  restaurado antes, porque a faixa nova pode exigir outro sample rate. Depois de um `load`
  bem-sucedido o device está sempre livre.
- **`play()` é quem sequestra.** Vindo de `Loaded`: hog, rate, formato físico, volume, ring
  buffer, thread produtora, IOProc, start. Vindo de `Paused`: apenas `AudioDeviceStart`. Vindo
  de `Finished`: o decodificador já se esgotou, então o arquivo é reaberto do início e o caminho
  é o mesmo de `Loaded` — tocar de novo é recomeçar, não retomar.
- **`pause()`**: `AudioDeviceStop`. O ring buffer não é tocado; o índice de leitura fica onde
  está.
- **`set_volume(scalar)`**: aplica no device se ele estiver adquirido; senão guarda como
  pendente e aplica na aquisição.
- **`snapshot()`**: lê só atômicos. Nunca bloqueia.
- **`shutdown()`**: para, junta a produtora e restaura o device.

### Concorrência: por que o snapshot não pega o mutex

Os comandos mutam o engine e portanto pegam um `Mutex`. O `snapshot()` **não pega**, porque a
UI o chama a 10 Hz e um `play()` segurando o mutex por centenas de milissegundos durante a
aquisição do hog congelaria a janela.

Os campos que a UI lê ficam fora do mutex, num bloco só de atômicos:

```rust
struct SharedStatus {
    state: AtomicU8,            // discriminante de PlayerState
    frames_rendered: AtomicU64, // incrementado pelo IOProc
    underruns: AtomicU64,
    total_frames: AtomicI64,
    volume_scalar: AtomicU32,   // f32 nos bits, via to_bits/from_bits
}
```

O tempo decorrido vem de `frames_rendered` — o que de fato saiu para o DAC — e não do que a
produtora leu do arquivo, que está até dois segundos adiantado por causa do ring buffer. O
valor exibido é limitado a `total_frames`, porque o último bloco do IOProc conta o silêncio de
preenchimento.

**O IOProc nunca encosta no mutex.** Continua fazendo só cópia e atômicos, como hoje. A
disciplina de tempo real não muda: nada de alocar, travar, imprimir ou chamar o sistema.

### Superfície uniffi

```rust
#[derive(uniffi::Enum)]
pub enum PlayerState { Idle, Loaded, Playing, Paused, Finished, Failed }

#[derive(uniffi::Record)]
pub struct Snapshot {
    pub state: PlayerState,
    pub elapsed_seconds: f64,
    pub total_seconds: f64,
    pub underruns: u64,
    pub volume_scalar: f32,
    pub error: Option<String>,
}

#[derive(uniffi::Record)]
pub struct TrackFormat {
    pub sample_rate: f64,
    pub bit_depth: u32,
    pub channels: u32,
    pub codec: String,
    pub device_name: String,
    pub hog_active: bool,
}

#[derive(uniffi::Error, Debug)]
pub enum PlayerError { Load { message: String }, Device { message: String }, State { message: String } }

#[derive(uniffi::Object)]
pub struct HogPlayer { /* Mutex<Engine> + Arc<SharedStatus> */ }

#[uniffi::export]
impl HogPlayer {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self>;
    pub fn load(&self, path: String) -> Result<TrackFormat, PlayerError>;
    pub fn play(&self) -> Result<(), PlayerError>;
    pub fn pause(&self) -> Result<(), PlayerError>;
    pub fn set_volume(&self, scalar: f32) -> Result<(), PlayerError>;
    pub fn snapshot(&self) -> Snapshot;
    pub fn shutdown(&self);
}
```

O `PlayerError` vira `throws` no Swift. `snapshot()` não falha: em erro, o estado é `Failed` e
a mensagem vem no campo `error`.

### Encerramento e restauração

O Swift chama `shutdown()` em `applicationWillTerminate`. O `Drop` do `Engine` é a rede de
segurança para os caminhos que não passam por ali.

Force-quit continua deixando o sample rate trocado, exatamente como a CLI hoje: o sistema
operacional solta o hog sozinho — verificado com `kill -9` —, mas não desfaz a mudança de rate.
O Mac não fica mudo; fica configurado na frequência da última faixa.

## Bit-perfect: invariantes e provas

### Invariantes

1. O IOProc só copia e zera. Nenhuma operação aritmética sobre amostra, em nenhum caminho.
2. Volume é escrito na propriedade do device, nunca nas amostras. No built-in deste Mac ele é
   aplicado no amplificador analógico.
3. Pause não mexe no ring buffer. O resume continua no byte exatamente seguinte.
4. `format.rs` e `ring.rs` não são modificados por este trabalho.

### Provas

- **`make verify` tem de continuar passando** nos três arquivos já validados, depois do
  refactor. É a prova existente: o ffmpeg extrai o PCM de referência, o player grava com
  `--dump` os bytes que iriam ao IOProc, e `tools/verify_bitperfect.py` compara amostra por
  amostra.
- **Novo — continuidade do pause.** `--dump --pause-at N` injeta pause e resume no meio do
  dump. A saída tem de ser byte-idêntica à do dump sem pause. Isso prova que o pause não
  descarta nem duplica bytes do ring, que é o risco concreto que ele introduz.

### O que deliberadamente não é testado automaticamente

Não haverá teste automatizado de "o volume não altera os bits". No modo dump o device nem chega
a ser adquirido, então esse teste passaria sempre, independentemente do código — teste que não
pode falhar não prova nada. A garantia aqui vem de duas coisas verificáveis por leitura e por
medição já feita: o caminho de dados contém apenas `memcpy`, e neste hardware o volume é
analógico.

## Metadados

Responsabilidade do Swift, em `HogPlayerKit`. O núcleo é uma função pura:

```swift
func trackMetadata(from items: [MetadataItem]) -> TrackMetadata
```

onde `MetadataItem` é `(keySpace: String, key: String, value: MetadataValue)`, extraído do
`AVAsset` pela camada de I/O. A separação existe para que a regra de mapeamento seja testável
sem arquivo.

Estratégia: se `commonMetadata` vier populado, usa; senão, cai na tabela por keyspace. Todas as
linhas abaixo foram medidas nesta máquina, com fixtures geradas por ffmpeg:

| Formato | `commonMetadata` | Keyspace cru | Título | Artista | Álbum | Capa |
|---|---|---|---|---|---|---|
| FLAC, Ogg | **vazio** | `vorb` | `TITLE` | `ARTIST` | `ALBUM` | `METADATA_BLOCK_PICTURE` |
| M4A, ALAC, AAC | funciona | `itsk` | `title` | `artist` | `albumName` | `artwork` |
| MP3 | funciona | `org.id3` | `TIT2` | `TPE1` | `TALB` | `APIC` |

Só o FLAC precisa do caminho alternativo — e é justamente o formato principal do projeto. Nos
demais, `commonMetadata` entrega inclusive a capa já pronta em bytes de imagem. No keyspace
`itsk` as chaves cruas são FourCC em inteiro com sinal (`-1452383891` no lugar de `©nam`), o que
é mais uma razão para usar `commonMetadata` ali em vez de mapear número.

Sem tags legíveis, o título é o nome do arquivo sem extensão e os demais campos ficam vazios.

A capa é `NSImage(data:)` sobre o `dataValue` do item, sem parsing intermediário.

## Interface

Janela única, tamanho fixo, aproximadamente 360×480. De cima para baixo: capa ou placeholder,
título, artista, álbum, `1:23 / 4:07`, botão play/pause, slider de volume com percentual, e uma
linha técnica no rodapé:

```
96 kHz · 24 bit · flac · hog ativo · 0 underruns
```

Carregar arquivo por três caminhos: `NSOpenPanel`, arrastar-e-soltar na janela, e argumento de
linha de comando — este último para iterar rápido durante o desenvolvimento.

### Volume e o teto de segurança

O teto continua valendo. No primeiro `play()`, se o device estiver acima de 50%, ele é baixado —
mesma regra da CLI — e o slider assume o valor real relido do hardware. Daí em diante o slider é
a fonte da verdade, com curso completo de 0 a 100%.

O máximo não é travado em 50%: o teto existe para evitar surpresa, não para limitar escolha
consciente. A justificativa original é que o fone recebe mais tensão do que deveria no volume
máximo, e o risco está em começar a tocar alto sem querer — não em o usuário decidir subir.

### ViewModel

`PlayerViewModel` é um `ObservableObject` que faz poll de `snapshot()` num timer de 10 Hz. A
tradução `Snapshot -> o que aparece na tela` é uma função pura, separada do timer, e portanto
testável sem hardware.

## Testes

### Rust

Os 50 testes atuais continuam valendo sem alteração, porque `format.rs`, `ring.rs` e `volume.rs`
não são tocados. Entram:

- **Transições puras**, em `transitions.rs`:
  `next_state(current: PlayerState, command: Command) -> Result<PlayerState, TransitionError>`.
  As regras ficam testáveis sem device; o efeito no hardware é a casca imperativa em volta.
  É o mesmo padrão que o projeto já usa — `format.rs` puro contra `device.rs` shell. Cobertura:
  todo par estado×comando, incluindo os inválidos (`pause` em `Idle`, `play` sem `load`).
- **Continuidade do pause** no modo dump, conforme a seção de provas.
- **`make verify`** como gate de regressão.

### Swift

`swift test`, sem Xcode:

- Mapeador de metadados, contra fixtures geradas por ffmpeg — um FLAC com tags completas, um
  FLAC sem tag nenhuma, um M4A e um MP3.
- Formatação de tempo: `0:07`, `1:23`, `59:59`, e acima de uma hora.
- Mapeamento `Snapshot -> estado de UI`, incluindo os casos de erro e de `Finished`.
- Um teste de integração que carrega um arquivo real pela fronteira uniffi e lê um snapshot,
  marcado como dependente de hardware.

## Ordem de build

Obrigatória, e forçada pelo Makefile:

```
cargo build --release   →  libhog_audio.a
uniffi-bindgen          →  hog_audio.swift + hog_audioFFI.h + modulemap
swift build             →  executável
make app                →  HogAudio.app
```

Rodar `swift build` isolado falha até os bindings existirem. O `Package.swift` usa
`linkerSettings: [.unsafeFlags([...])]` para apontar a staticlib, o que é permitido em pacote
raiz e impede que este pacote seja consumido como dependência de outro — irrelevante aqui.

## Fora de escopo

Seek, playlist, gapless, integração com o Now Playing do sistema, e qualquer fonte que não seja
arquivo local.

O `cpp/` fica congelado e intocado. Aposentá-lo é decisão separada, para depois de o caminho
Rust+Swift estar em pé.

## Riscos

- **O app não pode ser sandboxed.** Hog mode não sobrevive ao sandbox, o que fecha a porta da
  App Store. Não é objetivo do projeto.
- **`uniffi` é a segunda dependência do crate**, que até aqui tinha apenas `coreaudio-sys`. É o
  custo aceito por ter tipos e erros gerados no lado Swift em vez de escritos à mão.
- **O refactor do `run()` é o ponto de maior risco do trabalho**: ele mexe na ordem de operações
  que resolveu as armadilhas caras do projeto — hog antes do rate, espera pelo rate efetivar,
  releitura do formato efetivo, escrita de volume confirmada. `make verify` cobre o resultado
  em bytes, mas não a ordem; a ordem é preservada por leitura cuidadosa e por manter a CLI
  funcionando como está.
- **Tirar o fone durante a reprodução não é tratado nesta versão.** O device desaparece, o
  IOProc para de ser chamado e o engine fica em `Playing` com o tempo congelado. Detectar isso
  exige um listener de propriedade do HAL, que fica para uma etapa seguinte; hoje a saída é
  fechar o player. Está registrado por ser o cenário mais provável de acontecer na prática.
- **Pausado por muito tempo é o Mac mudo por muito tempo.** É consequência aceita da escolha de
  manter o hog no pause, e a linha técnica na UI mostra `hog ativo` justamente para que isso
  nunca seja surpresa.
