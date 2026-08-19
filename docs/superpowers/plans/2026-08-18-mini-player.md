# Mini Player (engine Rust + UI SwiftUI) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transformar a CLI `hog-audio` num mini player de faixa única com interface nativa — play/pause, volume, metadados, tempo e indicadores técnicos — sem perder a garantia de bit-perfect.

**Architecture:** O crate Rust vira biblioteca com máquina de estados (`Idle → Loaded → Playing ⇄ Paused → Finished`), exposta ao Swift por uniffi. O app SwiftUI linka essa biblioteca estaticamente e roda no mesmo processo, porque hog mode é por PID. A CLI continua existindo consumindo o mesmo engine, e é ela quem roda a prova de bit-perfect contra o oráculo do ffmpeg.

**Tech Stack:** Rust 1.93 (edition 2024), `coreaudio-sys 0.2.18`, `uniffi 0.32` (modo proc-macro, sem `.udl`), Swift 6.3 + SwiftUI, Swift Package Manager, AVFoundation para metadados.

**Spec:** `docs/superpowers/specs/2026-08-18-mini-player-design.md`

## Global Constraints

- **Idioma:** código, identificadores e nomes de arquivo em inglês; comentários, mensagens de UI/CLI e commits em pt-BR. Commits em Conventional Commits no infinitivo: `feat(engine): extrair orquestracao do run`.
- **Comentário só explica *por quê*,** nunca *o quê*. Não comentar o óbvio.
- **O IOProc é código de tempo real:** dentro dele, apenas `memcpy`, `write_bytes` e operações atômicas. Proibido alocar, travar mutex, imprimir, chamar o sistema ou entrar em pânico.
- **`format.rs`, `ring.rs` e `volume.rs` não podem ser modificados** por nenhuma task deste plano. Se uma task parecer exigir isso, pare e reporte.
- **Nenhuma amostra pode ser multiplicada, escalada ou filtrada** em nenhum ponto do caminho de dados.
- **Gate de regressão obrigatório**, exigido nominalmente pelas Tasks 1, 6 e 7:
  ```bash
  make verify FILE=testdata/t96_24.flac BITS=24
  ```
  As três comparações precisam reportar bit-perfect / dumps idênticos.
- **Linha de base já verificada** (rodada antes deste plano existir, na `main` em `69c9849`): 1.152.000 amostras, C++ e Rust ambos bit-perfect, dumps idênticos.
- **Toolchain confirmado nesta máquina:** rustc 1.93.1, Swift 6.3.1, Xcode 26.4.1, uniffi 0.32.0.
- **Trabalhar em branch própria**, nunca direto na `main`.

## Preparação (antes da Task 1)

`testdata/` é gitignored. Se estiver vazio, regenerar — e note o `-ac 2`: o filtro `sine` do
ffmpeg produz **mono**, e mono aciona o caminho de duplicação de canais, que desalinha a
comparação com a referência.

```bash
mkdir -p testdata
ffmpeg -v error -y -f lavfi -i "sine=frequency=1000:duration=6" -ac 2 -ar 96000 -sample_fmt s32 testdata/t96_24.flac
ffmpeg -v error -y -f lavfi -i "sine=frequency=1000:duration=6" -ac 2 -ar 44100 -sample_fmt s16 testdata/t44_16.flac
git checkout -b feat/mini-player
```

## Estrutura de arquivos

| Arquivo | Responsabilidade | Task |
|---|---|---|
| `rust/src/lib.rs` | Raiz da biblioteca; declara os módulos públicos | 1 |
| `rust/src/transitions.rs` | Regras de transição, puras, sem hardware | 2 |
| `rust/src/status.rs` | `SharedStatus`: os campos que a UI lê sem lock | 3 |
| `rust/src/playback.rs` | `Playback` + `io_proc`, movidos de `main.rs` | 4 |
| `rust/src/engine.rs` | `Engine`: a casca imperativa da máquina de estados | 5, 6 |
| `rust/src/api.rs` | Superfície uniffi consumida pelo Swift | 8 |
| `rust/src/bin/uniffi-bindgen.rs` | Binário gerador de bindings | 8 |
| `rust/src/main.rs` | CLI, agora consumindo o `Engine` | 1, 6, 7 |
| `apps/player/Package.swift` | Alvos SPM e linkagem da staticlib | 9 |
| `apps/player/Sources/HogPlayerKit/Metadata.swift` | Mapeamento de tags → `TrackMetadata` | 10 |
| `apps/player/Sources/HogPlayerKit/TimeFormat.swift` | `1:23`, `59:59`, `1:02:03` | 11 |
| `apps/player/Sources/HogPlayerKit/DisplayState.swift` | `Snapshot` → o que a tela mostra | 11 |
| `apps/player/Sources/HogPlayerKit/PlayerViewModel.swift` | Poll a 10 Hz e comandos | 12 |
| `apps/player/Sources/HogPlayer/HogPlayerApp.swift` | Entrada do app e encerramento | 13, 14 |
| `apps/player/Sources/HogPlayer/ContentView.swift` | A janela | 13 |
| `Makefile` | Alvos `rust-lib`, `bindings`, `swift-test`, `app` | 8, 9, 14 |

---

### Task 1: Converter o crate Rust em biblioteca + binário

Refatoração pura: nenhum comportamento muda. O objetivo é que exista uma biblioteca para o
Swift consumir mais adiante. O gate é o `make verify` continuar bit-perfect.

**Files:**
- Create: `rust/src/lib.rs`
- Modify: `rust/Cargo.toml`, `rust/src/main.rs:1-19`

**Interfaces:**
- Consumes: nada.
- Produces: crate de biblioteca `hog_audio` com os módulos públicos `device`, `ffi`, `format`, `ring`, `source`, `volume`. Todas as tasks seguintes importam a partir daí.

- [ ] **Step 1: Rodar a suíte atual e anotar o número de testes**

```bash
cd rust && cargo test 2>&1 | tail -5
```

Anote o total. Ele não pode diminuir em nenhuma task deste plano.

- [ ] **Step 2: Criar `rust/src/lib.rs`**

```rust
//! Núcleo do hog-audio: decodificação, negociação de formato, ring buffer e controle do
//! device em modo exclusivo. Tanto a CLI quanto a interface gráfica consomem esta
//! biblioteca; nenhuma das duas fala com o Core Audio diretamente.

pub mod device;
pub mod ffi;
pub mod format;
pub mod ring;
pub mod source;
pub mod volume;
```

- [ ] **Step 3: Declarar a biblioteca e o binário no `Cargo.toml`**

`crate-type` inclui `staticlib` porque o app Swift vai linkar o `.a` diretamente. Não
declaramos `cdylib`: foi verificado que o `uniffi-bindgen` lê a staticlib e produz saída
idêntica, e assim o bundle do app não precisa embarcar dylib nenhuma.

```toml
[package]
name = "hog-audio"
version = "0.1.0"
edition = "2024"

[lib]
name = "hog_audio"
crate-type = ["lib", "staticlib"]
path = "src/lib.rs"

[[bin]]
name = "hog-audio"
path = "src/main.rs"

[dependencies]
coreaudio-sys = "0.2.18"
```

- [ ] **Step 4: Trocar as declarações de módulo do `main.rs` por importações da biblioteca**

Substitua as linhas 1 a 19 de `rust/src/main.rs` por:

```rust
use std::cell::UnsafeCell;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::Arc;

use coreaudio_sys::*;

use hog_audio::device::{query_default_output_device, HoggedDevice, OutputDevice};
use hog_audio::format::{self, validate_interleaved_format, FileFormat};
use hog_audio::ring::{aligned_read_size, RingBuffer};
use hog_audio::source::AudioSource;
use hog_audio::volume::{apply_ceiling, parse_volume, VolumeRequest, VolumeUnit};
```

O `self` no import de `format` é necessário porque `run()` chama `format::negotiate(...)` pelo
caminho do módulo.

- [ ] **Step 5: Compilar e rodar os testes**

```bash
cd rust && cargo build --release && cargo test 2>&1 | tail -5
```

Esperado: compila sem aviso, e o total de testes é o mesmo do Step 1.

- [ ] **Step 6: Confirmar que a staticlib foi produzida**

```bash
ls -l rust/target/release/libhog_audio.a
```

Esperado: o arquivo existe.

- [ ] **Step 7: Rodar o gate de bit-perfect**

```bash
make verify FILE=testdata/t96_24.flac BITS=24
```

Esperado: as três comparações reportam BIT-PERFECT e "dumps identicos". Se qualquer uma
divergir, **pare**: uma refatoração puramente estrutural não pode mudar bytes, e algo foi
movido errado.

- [ ] **Step 8: Commit**

```bash
git add rust/Cargo.toml rust/src/lib.rs rust/src/main.rs
git commit -m "refactor(rust): expor o nucleo como biblioteca consumida pela cli"
```

---

### Task 2: Regras de transição puras

O coração da máquina de estados, sem hardware nenhum, para ser exaustivamente testável. O
efeito colateral no device fica na casca imperativa da Task 5.

**Files:**
- Create: `rust/src/transitions.rs`
- Modify: `rust/src/lib.rs`

**Interfaces:**
- Consumes: nada.
- Produces: `PlayerState` (`Idle`, `Loaded`, `Playing`, `Paused`, `Finished`, `Failed`), `Command` (`Load`, `Play`, `Pause`, `Stop`), `TransitionError { message: &'static str }`, e `next_state(current: PlayerState, command: Command) -> Result<PlayerState, TransitionError>`.

A tabela que a implementação precisa satisfazer:

| estado atual | `Load` | `Play` | `Pause` | `Stop` |
|---|---|---|---|---|
| `Idle` | `Loaded` | erro | erro | `Idle` |
| `Loaded` | `Loaded` | `Playing` | erro | `Idle` |
| `Playing` | `Loaded` | `Playing` | `Paused` | `Idle` |
| `Paused` | `Loaded` | `Playing` | `Paused` | `Idle` |
| `Finished` | `Loaded` | `Playing` | erro | `Idle` |
| `Failed` | `Loaded` | erro | erro | `Idle` |

`Play` em `Playing` e `Pause` em `Paused` são **idempotentes**, não erro: a interface tem um
botão só, e um duplo clique não pode virar caixa de diálogo de erro. `Play` em `Failed` é erro
porque `Failed` significa que o device foi solto e a fonte pode estar inválida — é preciso
carregar de novo.

- [ ] **Step 1: Escrever os testes que falham**

Crie `rust/src/transitions.rs` contendo **apenas** o bloco de testes abaixo:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_e_aceito_em_qualquer_estado() {
        for estado in [
            PlayerState::Idle,
            PlayerState::Loaded,
            PlayerState::Playing,
            PlayerState::Paused,
            PlayerState::Finished,
            PlayerState::Failed,
        ] {
            assert_eq!(
                next_state(estado, Command::Load),
                Ok(PlayerState::Loaded),
                "load deveria ser aceito em {estado:?}"
            );
        }
    }

    #[test]
    fn stop_volta_para_idle_de_qualquer_estado() {
        for estado in [
            PlayerState::Idle,
            PlayerState::Loaded,
            PlayerState::Playing,
            PlayerState::Paused,
            PlayerState::Finished,
            PlayerState::Failed,
        ] {
            assert_eq!(next_state(estado, Command::Stop), Ok(PlayerState::Idle));
        }
    }

    #[test]
    fn play_toca_a_partir_de_loaded_paused_e_finished() {
        for estado in [PlayerState::Loaded, PlayerState::Paused, PlayerState::Finished] {
            assert_eq!(
                next_state(estado, Command::Play),
                Ok(PlayerState::Playing),
                "play deveria tocar a partir de {estado:?}"
            );
        }
    }

    #[test]
    fn play_sem_arquivo_carregado_e_erro() {
        assert!(next_state(PlayerState::Idle, Command::Play).is_err());
        assert!(next_state(PlayerState::Failed, Command::Play).is_err());
    }

    #[test]
    fn play_durante_a_reproducao_e_idempotente() {
        assert_eq!(
            next_state(PlayerState::Playing, Command::Play),
            Ok(PlayerState::Playing)
        );
    }

    #[test]
    fn pause_so_faz_sentido_tocando_ou_ja_pausado() {
        assert_eq!(
            next_state(PlayerState::Playing, Command::Pause),
            Ok(PlayerState::Paused)
        );
        assert_eq!(
            next_state(PlayerState::Paused, Command::Pause),
            Ok(PlayerState::Paused)
        );
        for estado in [
            PlayerState::Idle,
            PlayerState::Loaded,
            PlayerState::Finished,
            PlayerState::Failed,
        ] {
            assert!(
                next_state(estado, Command::Pause).is_err(),
                "pause deveria falhar em {estado:?}"
            );
        }
    }

    #[test]
    fn o_erro_traz_mensagem_util() {
        let erro = next_state(PlayerState::Idle, Command::Play).unwrap_err();
        assert!(
            erro.message.contains("carregado"),
            "mensagem pouco informativa: {}",
            erro.message
        );
    }
}
```

- [ ] **Step 2: Declarar o módulo e rodar os testes para vê-los falhar**

Adicione `pub mod transitions;` em `rust/src/lib.rs`, em ordem alfabética entre `source` e
`volume`. Depois:

```bash
cd rust && cargo test transitions 2>&1 | tail -20
```

Esperado: FALHA de compilação — `cannot find type PlayerState in this scope`. É o vermelho
correto: os testes existem e o código não.

- [ ] **Step 3: Implementar**

Insira **acima** do bloco `#[cfg(test)]` em `rust/src/transitions.rs`:

```rust
//! Regras da máquina de estados do player, isoladas de qualquer efeito no hardware.
//! Manter estas regras puras é o que torna possível cobrir todos os pares estado × comando
//! sem device, sem arquivo e sem thread.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerState {
    Idle,
    Loaded,
    Playing,
    Paused,
    Finished,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Load,
    Play,
    Pause,
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitionError {
    pub message: &'static str,
}

pub fn next_state(
    current: PlayerState,
    command: Command,
) -> Result<PlayerState, TransitionError> {
    use Command::*;
    use PlayerState::*;

    match (current, command) {
        // Carregar substitui o que estiver carregado, venha de onde vier.
        (_, Load) => Ok(Loaded),
        // Encerrar sempre devolve ao repouso.
        (_, Stop) => Ok(Idle),

        (Loaded | Paused | Finished, Play) => Ok(Playing),
        // Idempotente de propósito: a interface tem um botão só, e um duplo clique não pode
        // virar mensagem de erro.
        (Playing, Play) => Ok(Playing),
        (Idle, Play) => Err(TransitionError {
            message: "nenhum arquivo carregado",
        }),
        (Failed, Play) => Err(TransitionError {
            message: "houve uma falha; carregue o arquivo de novo",
        }),

        (Playing, Pause) => Ok(Paused),
        (Paused, Pause) => Ok(Paused),
        (Idle | Loaded | Finished | Failed, Pause) => Err(TransitionError {
            message: "não está tocando",
        }),
    }
}
```

- [ ] **Step 4: Rodar os testes e vê-los passar**

```bash
cd rust && cargo test transitions 2>&1 | tail -12
```

Esperado: 7 testes passando.

- [ ] **Step 5: Provar que os testes conseguem reprovar**

Um teste que nunca falhou não provou nada. Troque temporariamente a linha
`(Playing, Pause) => Ok(Paused),` por `(Playing, Pause) => Ok(Playing),` e rode de novo.

```bash
cd rust && cargo test transitions 2>&1 | tail -12
```

Esperado: `pause_so_faz_sentido_tocando_ou_ja_pausado` **falha**. Desfaça a mutação e confirme
que volta ao verde.

- [ ] **Step 6: Commit**

```bash
git add rust/src/transitions.rs rust/src/lib.rs
git commit -m "feat(engine): adicionar regras puras de transicao de estado"
```

---

### Task 3: `SharedStatus` — o que a interface lê sem travar

A interface consulta o estado dez vezes por segundo. Se essa leitura disputasse o mesmo mutex
dos comandos, um `play()` segurando o lock durante a aquisição do hog — que leva centenas de
milissegundos — congelaria a janela. Por isso estes campos vivem fora do mutex, em atômicos.

**Files:**
- Create: `rust/src/status.rs`
- Modify: `rust/src/lib.rs`

**Interfaces:**
- Consumes: `PlayerState` de `transitions.rs`.
- Produces: `SharedStatus` com `new()`, `set_state(PlayerState)`, `state() -> PlayerState`, `add_frames(u64)`, `reset_progress()`, `set_track(total_frames: i64, sample_rate: f64)`, `elapsed_seconds() -> f64`, `total_seconds() -> f64`, `add_underrun()`, `underruns() -> u64`, `set_volume(f32)`, `volume() -> f32`.

- [ ] **Step 1: Escrever os testes que falham**

Crie `rust/src/status.rs` contendo **apenas**:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_estado_sobrevive_a_ida_e_volta_para_u8() {
        // Um mapeamento errado aqui é silencioso: a interface mostraria "pausado" com o
        // player tocando, sem nenhum erro em lugar nenhum.
        for estado in [
            PlayerState::Idle,
            PlayerState::Loaded,
            PlayerState::Playing,
            PlayerState::Paused,
            PlayerState::Finished,
            PlayerState::Failed,
        ] {
            let status = SharedStatus::new();
            status.set_state(estado);
            assert_eq!(status.state(), estado);
        }
    }

    #[test]
    fn comeca_em_idle_e_zerado() {
        let status = SharedStatus::new();
        assert_eq!(status.state(), PlayerState::Idle);
        assert_eq!(status.elapsed_seconds(), 0.0);
        assert_eq!(status.underruns(), 0);
    }

    #[test]
    fn frames_viram_segundos_pela_taxa_de_amostragem() {
        let status = SharedStatus::new();
        status.set_track(96_000 * 10, 96_000.0);
        status.add_frames(48_000);
        assert!((status.elapsed_seconds() - 0.5).abs() < 1e-9);
        assert!((status.total_seconds() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn o_decorrido_nunca_passa_da_duracao_total() {
        // O último bloco do IOProc conta o silêncio de preenchimento; sem o teto a interface
        // mostraria a faixa passando do fim.
        let status = SharedStatus::new();
        status.set_track(1_000, 1_000.0);
        status.add_frames(5_000);
        assert!((status.elapsed_seconds() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sem_faixa_carregada_o_decorrido_e_zero() {
        let status = SharedStatus::new();
        status.add_frames(500);
        assert_eq!(status.elapsed_seconds(), 0.0);
    }

    #[test]
    fn reset_zera_o_progresso_mas_preserva_a_faixa() {
        let status = SharedStatus::new();
        status.set_track(96_000, 96_000.0);
        status.add_frames(48_000);
        status.add_underrun();
        status.reset_progress();
        assert_eq!(status.elapsed_seconds(), 0.0);
        assert_eq!(status.underruns(), 0);
        assert!((status.total_seconds() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn o_volume_sobrevive_a_ida_e_volta() {
        let status = SharedStatus::new();
        status.set_volume(0.375);
        assert!((status.volume() - 0.375).abs() < 1e-6);
    }
}
```

- [ ] **Step 2: Declarar o módulo e ver os testes falharem**

Adicione `pub mod status;` a `rust/src/lib.rs`. Depois:

```bash
cd rust && cargo test status 2>&1 | tail -15
```

Esperado: FALHA de compilação — `cannot find type SharedStatus in this scope`.

- [ ] **Step 3: Implementar**

Insira acima do bloco de testes em `rust/src/status.rs`:

```rust
//! Os campos que a interface consulta continuamente. Ficam fora do mutex do engine de
//! propósito: a interface lê dez vezes por segundo, e um comando lento — a aquisição do hog
//! leva centenas de milissegundos — travaria a janela se disputasse o mesmo lock.
//!
//! O IOProc escreve `frames_rendered` e `underruns` daqui. Por isso tudo é atômico: são as
//! únicas operações que um callback de tempo real pode fazer com segurança.

use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, AtomicU8, Ordering};

use crate::transitions::PlayerState;

pub struct SharedStatus {
    state: AtomicU8,
    frames_rendered: AtomicU64,
    underruns: AtomicU64,
    total_frames: AtomicI64,
    sample_rate_bits: AtomicU64,
    volume_bits: AtomicU32,
}

fn state_to_u8(state: PlayerState) -> u8 {
    match state {
        PlayerState::Idle => 0,
        PlayerState::Loaded => 1,
        PlayerState::Playing => 2,
        PlayerState::Paused => 3,
        PlayerState::Finished => 4,
        PlayerState::Failed => 5,
    }
}

fn state_from_u8(value: u8) -> PlayerState {
    match value {
        1 => PlayerState::Loaded,
        2 => PlayerState::Playing,
        3 => PlayerState::Paused,
        4 => PlayerState::Finished,
        5 => PlayerState::Failed,
        _ => PlayerState::Idle,
    }
}

impl SharedStatus {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(state_to_u8(PlayerState::Idle)),
            frames_rendered: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            total_frames: AtomicI64::new(0),
            sample_rate_bits: AtomicU64::new(0f64.to_bits()),
            volume_bits: AtomicU32::new(0f32.to_bits()),
        }
    }

    pub fn set_state(&self, state: PlayerState) {
        self.state.store(state_to_u8(state), Ordering::Release);
    }

    pub fn state(&self) -> PlayerState {
        state_from_u8(self.state.load(Ordering::Acquire))
    }

    pub fn set_track(&self, total_frames: i64, sample_rate: f64) {
        self.total_frames.store(total_frames, Ordering::Release);
        self.sample_rate_bits
            .store(sample_rate.to_bits(), Ordering::Release);
        self.reset_progress();
    }

    pub fn reset_progress(&self) {
        self.frames_rendered.store(0, Ordering::Release);
        self.underruns.store(0, Ordering::Release);
    }

    /// Chamado pelo IOProc. `Relaxed` basta: ninguém sincroniza dados com este contador.
    pub fn add_frames(&self, frames: u64) {
        self.frames_rendered.fetch_add(frames, Ordering::Relaxed);
    }

    pub fn elapsed_seconds(&self) -> f64 {
        let rate = f64::from_bits(self.sample_rate_bits.load(Ordering::Acquire));
        if rate <= 0.0 {
            return 0.0;
        }
        let elapsed = self.frames_rendered.load(Ordering::Relaxed) as f64 / rate;
        let total = self.total_seconds();
        if total > 0.0 && elapsed > total {
            total
        } else {
            elapsed
        }
    }

    pub fn total_seconds(&self) -> f64 {
        let rate = f64::from_bits(self.sample_rate_bits.load(Ordering::Acquire));
        if rate <= 0.0 {
            return 0.0;
        }
        self.total_frames.load(Ordering::Acquire) as f64 / rate
    }

    pub fn add_underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }

    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    pub fn set_volume(&self, scalar: f32) {
        self.volume_bits.store(scalar.to_bits(), Ordering::Release);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Acquire))
    }
}

impl Default for SharedStatus {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 4: Rodar os testes e vê-los passar**

```bash
cd rust && cargo test status 2>&1 | tail -12
```

Esperado: 7 testes passando.

- [ ] **Step 5: Provar que o teste do mapeamento consegue reprovar**

Troque em `state_from_u8` a linha `3 => PlayerState::Paused,` por `3 => PlayerState::Playing,`
e rode:

```bash
cd rust && cargo test status 2>&1 | tail -12
```

Esperado: `todo_estado_sobrevive_a_ida_e_volta_para_u8` **falha**. Desfaça e confirme o verde.

- [ ] **Step 6: Commit**

```bash
git add rust/src/status.rs rust/src/lib.rs
git commit -m "feat(engine): adicionar estado compartilhado sem lock para a interface"
```

---

### Task 4: Mover `Playback` e o IOProc para a biblioteca

Hoje o `Playback` e o `io_proc` vivem em `main.rs`, fora do alcance da interface. Esta task os
move para a biblioteca sem alterar a lógica, e acrescenta a contagem de frames que alimenta o
tempo decorrido.

**Files:**
- Create: `rust/src/playback.rs`
- Modify: `rust/src/lib.rs`, `rust/src/main.rs`

**Interfaces:**
- Consumes: `RingBuffer` e `aligned_read_size` de `ring.rs`; `SharedStatus` de `status.rs`.
- Produces: `Playback` com os campos públicos `ring`, `producer_done`, `finished`, `status`, `bytes_per_frame`, `bytes_per_sample`, `channels`, `non_interleaved`, `scratch`; o construtor `Playback::new(capacity_bytes: usize, client: &AudioStreamBasicDescription, non_interleaved: bool, status: Arc<SharedStatus>) -> Playback`; e `io_proc` com a assinatura de `AudioDeviceIOProc`.

- [ ] **Step 1: Criar `rust/src/playback.rs` movendo o código existente**

Copie de `rust/src/main.rs` o `struct Playback` (linhas 44 a 60), os dois `unsafe impl` e a
função `io_proc` inteira (linhas 63 a 145). Coloque no arquivo novo, com este cabeçalho e as
importações:

```rust
//! Estado compartilhado entre a thread que decodifica e o IOProc de tempo real, mais o
//! próprio callback.
//!
//! Tudo que roda dentro do `io_proc` obedece à disciplina de tempo real: só cópia de memória
//! e operações atômicas. Alocar, travar um mutex ou imprimir aqui produz falhas audíveis e
//! intermitentes, que não aparecem em teste.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use coreaudio_sys::*;

use crate::ring::{aligned_read_size, RingBuffer};
use crate::status::SharedStatus;
```

Faça três alterações no que foi movido:

1. Torne `pub` o `struct Playback` e todos os seus campos.
2. Troque o campo `underruns: AtomicU64` por `pub status: Arc<SharedStatus>`.
3. Substitua as três ocorrências de `p.underruns.fetch_add(1, Ordering::Relaxed);` por
   `p.status.add_underrun();`.

- [ ] **Step 2: Contar os frames entregues ao device**

É daqui que sai o tempo decorrido mostrado na interface — do que de fato chegou ao DAC, e não
do que a produtora leu do arquivo, que está até dois segundos adiantado por causa do ring
buffer.

No `io_proc`, no ramo intercalado, logo após `let got = p.ring.read(&mut dst[..take]);`
acrescente:

```rust
p.status.add_frames((need / p.bytes_per_frame as usize) as u64);
```

No ramo não-intercalado, logo após `let take = aligned_read_size(...)` e o `read` correspondente,
acrescente:

```rust
p.status.add_frames(frames as u64);
```

Conta-se o que foi pedido, não o que foi lido, porque o bloco final completado com silêncio
também consome tempo real de reprodução. O teto contra ultrapassar a duração já está em
`SharedStatus::elapsed_seconds`.

- [ ] **Step 3: Acrescentar o construtor**

Ao final de `rust/src/playback.rs`, antes de qualquer bloco de teste:

```rust
impl Playback {
    pub fn new(
        capacity_bytes: usize,
        client: &AudioStreamBasicDescription,
        non_interleaved: bool,
        status: Arc<SharedStatus>,
    ) -> Self {
        Self {
            ring: RingBuffer::new(capacity_bytes),
            producer_done: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            status,
            bytes_per_frame: client.mBytesPerFrame,
            bytes_per_sample: client.mBytesPerFrame / client.mChannelsPerFrame,
            channels: client.mChannelsPerFrame,
            non_interleaved,
            scratch: UnsafeCell::new(vec![0u8; capacity_bytes]),
        }
    }
}
```

- [ ] **Step 4: Declarar o módulo e apontar o `main.rs` para ele**

Adicione `pub mod playback;` a `rust/src/lib.rs`. Em `rust/src/main.rs`: apague o `struct
Playback`, os dois `unsafe impl` e a função `io_proc`; acrescente
`use hog_audio::playback::{io_proc, Playback};` e `use hog_audio::status::SharedStatus;`.

No `run()`, troque a construção literal do `Playback` pelo construtor, criando antes o status:

```rust
let status = Arc::new(SharedStatus::new());
status.set_track(source.total_frames(), client.mSampleRate);
let playback = Arc::new(Playback::new(
    ring_bytes,
    &client,
    non_interleaved,
    Arc::clone(&status),
));
```

E troque a leitura final do contador de underruns:

```rust
let underruns = status.underruns();
```

- [ ] **Step 5: Compilar e rodar todos os testes**

```bash
cd rust && cargo build --release && cargo test 2>&1 | tail -5
```

Esperado: compila e o total de testes segue igual ao da Task 1.

- [ ] **Step 6: Rodar o gate de bit-perfect**

```bash
make verify FILE=testdata/t96_24.flac BITS=24
```

Esperado: três comparações bit-perfect. O IOProc foi mexido nesta task, então este gate é o que
prova que a mexida não alterou byte nenhum.

- [ ] **Step 7: Commit**

```bash
git add rust/src/playback.rs rust/src/lib.rs rust/src/main.rs
git commit -m "refactor(playback): mover ioproc para biblioteca e contar frames"
```

---

### Task 5: `Engine` e o comando `load`

O `load` deliberadamente **não adquire o device**. Ele abre o arquivo, consulta as capacidades
do device — leitura pura — e negocia. Assim um arquivo de 192 kHz num device que não suporta
falha com o Mac ainda intacto, e ninguém fica mudo por abrir um arquivo.

**Files:**
- Create: `rust/src/engine.rs`
- Modify: `rust/src/lib.rs`

**Interfaces:**
- Consumes: `PlayerState`, `Command`, `next_state` de `transitions.rs`; `SharedStatus` de `status.rs`; `AudioSource` de `source.rs`; `query_default_output_device`, `OutputDevice`, `HoggedDevice` de `device.rs`; `Decision`, `negotiate` de `format.rs`; `VolumeRequest` de `volume.rs`.
- Produces: `Engine` com `new()`, `status() -> Arc<SharedStatus>`, `state() -> PlayerState`, `load(&self, path: &str) -> Result<LoadedTrack, String>`; e `LoadedTrack { sample_rate: f64, bit_depth: u32, channels: u32, codec: String, device_name: String, total_seconds: f64 }`.

- [ ] **Step 1: Escrever os testes que falham**

Crie `rust/src/engine.rs` contendo **apenas**:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_novo_comeca_em_idle() {
        let engine = Engine::new();
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    fn arquivo_inexistente_falha_sem_mudar_de_estado() {
        // O arquivo é aberto antes de qualquer consulta ao device: um caminho inválido não
        // pode chegar perto do hardware.
        let engine = Engine::new();
        let erro = engine.load("/tmp/nao-existe-mesmo-12345.flac").unwrap_err();
        assert!(!erro.is_empty());
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    #[ignore = "precisa de um device de saída real"]
    fn load_de_flac_valido_vai_para_loaded() {
        let path = "../testdata/t96_24.flac";
        if !std::path::Path::new(path).exists() {
            eprintln!("pulando: {path} não existe (gere com ffmpeg)");
            return;
        }
        let engine = Engine::new();
        let track = engine.load(path).expect("deveria carregar");

        assert_eq!(engine.state(), PlayerState::Loaded);
        assert_eq!(track.sample_rate, 96000.0);
        assert_eq!(track.bit_depth, 24);
        assert_eq!(track.channels, 2);
        assert_eq!(track.codec, "flac");
        assert!(!track.device_name.is_empty());
        assert!((track.total_seconds - 6.0).abs() < 0.1);
        assert!((engine.status().total_seconds() - 6.0).abs() < 0.1);
    }

    #[test]
    #[ignore = "precisa de um device de saída real"]
    fn carregar_de_novo_substitui_a_faixa_anterior() {
        let a = "../testdata/t96_24.flac";
        let b = "../testdata/t44_16.flac";
        if !std::path::Path::new(a).exists() || !std::path::Path::new(b).exists() {
            eprintln!("pulando: testdata ausente");
            return;
        }
        let engine = Engine::new();
        engine.load(a).expect("deveria carregar o primeiro");
        let track = engine.load(b).expect("deveria carregar o segundo");
        assert_eq!(track.sample_rate, 44100.0);
        assert_eq!(engine.state(), PlayerState::Loaded);
    }
}
```

- [ ] **Step 2: Declarar o módulo e ver os testes falharem**

Adicione `pub mod engine;` a `rust/src/lib.rs`, e rode:

```bash
cd rust && cargo test engine 2>&1 | tail -15
```

Esperado: FALHA de compilação — `cannot find type Engine in this scope`.

- [ ] **Step 3: Implementar o `Engine` e o `load`**

Insira acima do bloco de testes em `rust/src/engine.rs`:

```rust
//! A casca imperativa da máquina de estados: aqui moram os efeitos sobre o hardware, o
//! arquivo e as threads. As regras de qual transição é permitida ficam em `transitions`,
//! puras e testáveis sem nada disso.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use coreaudio_sys::AudioStreamBasicDescription;

use crate::device::{query_default_output_device, HoggedDevice, OutputDevice};
use crate::format::{self, Decision};
use crate::playback::Playback;
use crate::source::AudioSource;
use crate::status::SharedStatus;
use crate::transitions::{next_state, Command, PlayerState};
use crate::volume::{VolumeRequest, VolumeUnit};

/// O que a interface precisa saber sobre a faixa recém-carregada.
pub struct LoadedTrack {
    pub sample_rate: f64,
    pub bit_depth: u32,
    pub channels: u32,
    pub codec: String,
    pub device_name: String,
    pub total_seconds: f64,
}

pub struct Engine {
    status: Arc<SharedStatus>,
    inner: Mutex<EngineInner>,
}

struct EngineInner {
    state: PlayerState,
    path: Option<String>,
    source: Option<AudioSource>,
    device: Option<OutputDevice>,
    decision: Option<Decision>,
}

/// O mesmo padrão da CLI: sem `--volume` explícito, o device é baixado para no máximo isto.
pub const DEFAULT_CEILING: f64 = 0.5;

impl Engine {
    pub fn new() -> Self {
        Self {
            status: Arc::new(SharedStatus::new()),
            inner: Mutex::new(EngineInner {
                state: PlayerState::Idle,
                path: None,
                source: None,
                device: None,
                decision: None,
                client: None,
                hogged: None,
                playback: None,
                producer: None,
                stop_producer: Arc::new(AtomicBool::new(false)),
                volume: None,
                ceiling: DEFAULT_CEILING,
            }),
        }
    }

    /// Um pânico dentro de um comando não pode deixar o player inutilizável para sempre: o
    /// estado interno é recuperado em vez de propagar o envenenamento do mutex.
    fn lock(&self) -> MutexGuard<'_, EngineInner> {
        self.inner.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    pub fn status(&self) -> Arc<SharedStatus> {
        Arc::clone(&self.status)
    }

    pub fn state(&self) -> PlayerState {
        self.lock().state
    }

    /// Define o volume que será aplicado quando o device for adquirido. `None` significa
    /// deixar o volume atual, respeitando o teto.
    pub fn set_requested_volume(&self, volume: Option<VolumeRequest>, ceiling: f64) {
        let mut inner = self.lock();
        inner.volume = volume;
        inner.ceiling = ceiling;
    }

    pub fn load(&self, path: &str) -> Result<LoadedTrack, String> {
        // Abrir o arquivo primeiro, antes de descartar o que estava carregado: se o caminho
        // for inválido, o player continua exatamente como estava.
        let source = AudioSource::open(path)?;
        let file_format = source.format();
        let codec = source.codec_name().to_string();
        let total_frames = source.total_frames();

        let mut inner = self.lock();
        self.teardown(&mut inner);

        let device = query_default_output_device()?;
        let decision = format::negotiate(&file_format, &device.caps);
        if !decision.play {
            return Err(decision.reason);
        }

        let device_name = device.name.clone();
        let target = next_state(inner.state, Command::Load).map_err(|e| e.message.to_string())?;

        inner.path = Some(path.to_string());
        inner.source = Some(source);
        inner.device = Some(device);
        inner.decision = Some(decision);
        inner.state = target;

        self.status.set_track(total_frames, file_format.sample_rate);
        self.status.set_state(target);

        Ok(LoadedTrack {
            sample_rate: file_format.sample_rate,
            bit_depth: file_format.bit_depth,
            channels: file_format.channels,
            codec,
            device_name,
            total_seconds: self.status.total_seconds(),
        })
    }

    /// Descarta o que estiver carregado. A Task 6 estende isto para também parar as threads e
    /// devolver o device.
    fn teardown(&self, inner: &mut EngineInner) {
        inner.source = None;
        inner.decision = None;
        inner.device = None;
        inner.client = None;
        inner.path = None;
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}


```

- [ ] **Step 4: Rodar os testes que não precisam de hardware**

```bash
cd rust && cargo test engine 2>&1 | tail -12
```

Esperado: 2 passando, 2 ignorados.

- [ ] **Step 5: Rodar os testes de hardware**

```bash
cd rust && cargo test engine -- --ignored --test-threads=1 2>&1 | tail -12
```

Esperado: 2 passando. Se falharem por não achar device, confira se há saída de áudio ativa.

- [ ] **Step 6: Commit**

```bash
git add rust/src/engine.rs rust/src/lib.rs
git commit -m "feat(engine): adicionar carga de faixa sem tocar no device"
```

---

### Task 6: `play`, `pause`, `volume` e `shutdown`; CLI reescrita sobre o engine

A task mais delicada do plano. Ela mexe na ordem de operações que resolveu as armadilhas caras
do projeto — hog antes do rate, espera pelo rate efetivar, releitura do formato efetivo,
escrita de volume confirmada. **Preserve a ordem exatamente como está em `run()`.**

**Files:**
- Modify: `rust/src/device.rs` (acrescentar `resume`), `rust/src/engine.rs`, `rust/src/main.rs`

**Interfaces:**
- Consumes: tudo da Task 5.
- Produces: `Engine::play(&self) -> Result<(), String>`, `Engine::pause(&self) -> Result<(), String>`, `Engine::set_volume(&self, scalar: f32) -> Result<(), String>`, `Engine::shutdown(&self) -> Result<(), String>`, `Engine::poll_finished(&self)`; e `HoggedDevice::resume(&mut self) -> Result<(), String>`.

- [ ] **Step 1: Acrescentar `resume` ao `HoggedDevice`**

Isto é uma armadilha concreta: o `start()` existente chama `AudioDeviceCreateIOProcID` **toda
vez**. Usá-lo para retomar registraria um segundo callback, com o primeiro ainda vivo — dois
consumidores disputando o mesmo ring buffer, e áudio picotado que só aparece depois do
primeiro pause.

Em `rust/src/device.rs`, logo após o método `stop`:

```rust
    /// Retoma sem recriar o IOProc. Chamar `start` de novo registraria um callback adicional
    /// e o anterior continuaria vivo, com os dois consumindo o mesmo ring buffer.
    pub fn resume(&mut self) -> Result<(), String> {
        if self.running {
            return Ok(());
        }
        if self.proc_id.is_none() {
            return Err("não há callback registrado para retomar".to_string());
        }
        let status = unsafe { AudioDeviceStart(self.device_id, self.proc_id) };
        if status != 0 {
            return Err(format!(
                "não consegui retomar a reprodução: {}",
                os_status_text(status)
            ));
        }
        self.running = true;
        Ok(())
    }
```

- [ ] **Step 2: Escrever os testes que falham**

Acrescente ao bloco `mod tests` de `rust/src/engine.rs`:

```rust
    #[test]
    fn play_sem_carregar_e_erro() {
        let engine = Engine::new();
        assert!(engine.play().is_err());
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    fn pause_sem_tocar_e_erro() {
        let engine = Engine::new();
        assert!(engine.pause().is_err());
    }

    #[test]
    fn shutdown_de_engine_parado_e_inofensivo() {
        let engine = Engine::new();
        assert!(engine.shutdown().is_ok());
        assert_eq!(engine.state(), PlayerState::Idle);
    }

    #[test]
    #[ignore = "precisa de um device de saída real; toma o device por alguns segundos"]
    fn ciclo_play_pause_play_shutdown() {
        let path = "../testdata/t96_24.flac";
        if !std::path::Path::new(path).exists() {
            eprintln!("pulando: {path} não existe (gere com ffmpeg)");
            return;
        }
        let engine = Engine::new();
        engine.load(path).expect("deveria carregar");

        engine.play().expect("deveria tocar");
        assert_eq!(engine.state(), PlayerState::Playing);
        std::thread::sleep(std::time::Duration::from_millis(400));
        let decorrido = engine.status().elapsed_seconds();
        assert!(decorrido > 0.0, "o tempo decorrido deveria avançar tocando");

        engine.pause().expect("deveria pausar");
        assert_eq!(engine.state(), PlayerState::Paused);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let parado = engine.status().elapsed_seconds();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(
            (engine.status().elapsed_seconds() - parado).abs() < 1e-6,
            "o tempo não pode avançar com o player pausado"
        );

        engine.play().expect("deveria retomar");
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(
            engine.status().elapsed_seconds() > parado,
            "o tempo deveria voltar a avançar depois de retomar"
        );

        engine.shutdown().expect("deveria restaurar o device");
        assert_eq!(engine.state(), PlayerState::Idle);
    }
```

- [ ] **Step 3: Ver os testes falharem**

```bash
cd rust && cargo test engine 2>&1 | tail -15
```

Esperado: FALHA de compilação — `no method named play found`.

- [ ] **Step 3b: Estender o `EngineInner` com o que só agora passa a ser usado**

A Task 5 deixou a struct enxuta de propósito: campos escritos e nunca lidos viram aviso de
`dead_code`. Agora eles passam a ter uso. Acrescente ao `EngineInner`:

```rust
    client: Option<AudioStreamBasicDescription>,
    hogged: Option<HoggedDevice>,
    playback: Option<Arc<Playback>>,
    producer: Option<JoinHandle<()>>,
    stop_producer: Arc<AtomicBool>,
    volume: Option<VolumeRequest>,
    ceiling: f64,
    volume_outcome: Option<VolumeOutcome>,
```

e ao inicializador de `Engine::new`, na mesma ordem: `client: None`, `hogged: None`,
`playback: None`, `producer: None`, `stop_producer: Arc::new(AtomicBool::new(false))`,
`volume: None`, `ceiling: DEFAULT_CEILING`, `volume_outcome: None`.

Acrescente também, no mesmo arquivo:

```rust
/// O mesmo padrão da CLI: sem `--volume` explícito, o device é baixado para no máximo isto.
pub const DEFAULT_CEILING: f64 = 0.5;
```

e o método que a CLI usa para dizer o volume desejado antes de tocar:

```rust
    /// Define o volume aplicado quando o device for adquirido. `None` mantém o volume atual,
    /// respeitando o teto.
    pub fn set_requested_volume(&self, volume: Option<VolumeRequest>, ceiling: f64) {
        let mut inner = self.lock();
        inner.volume = volume;
        inner.ceiling = ceiling;
    }
```

E, ao final do arquivo, a rede de segurança que a Task 5 não pôde declarar porque
`stop_and_release` ainda não existia:

```rust
impl Drop for Engine {
    /// Rede de segurança para os caminhos que não passam por `shutdown` — uma exceção não
    /// tratada no app, por exemplo. Não cobre `kill -9`: nesse caso o sistema solta o hog
    /// sozinho, mas o sample rate fica trocado.
    fn drop(&mut self) {
        let mut inner = self.lock();
        let _ = self.stop_and_release(&mut inner);
    }
}
```

- [ ] **Step 4: Implementar os comandos**

Acrescente ao `impl Engine` de `rust/src/engine.rs`:

```rust
    pub fn play(&self) -> Result<(), String> {
        let mut inner = self.lock();
        let target = next_state(inner.state, Command::Play).map_err(|e| e.message.to_string())?;

        match inner.state {
            PlayerState::Playing => return Ok(()), // idempotente: duplo clique no botão
            PlayerState::Paused => {
                let hogged = inner
                    .hogged
                    .as_mut()
                    .ok_or_else(|| "pausado sem device; recarregue a faixa".to_string())?;
                hogged.resume()?;
            }
            _ => self.start_common(&mut inner, true)?,
        }

        inner.state = target;
        self.status.set_state(target);
        Ok(())
    }

    pub fn pause(&self) -> Result<(), String> {
        let mut inner = self.lock();
        let target = next_state(inner.state, Command::Pause).map_err(|e| e.message.to_string())?;
        if let Some(hogged) = inner.hogged.as_mut() {
            hogged.stop();
        }
        inner.state = target;
        self.status.set_state(target);
        Ok(())
    }

    pub fn set_volume(&self, scalar: f32) -> Result<(), String> {
        let mut inner = self.lock();
        match inner.hogged.as_mut() {
            Some(hogged) => {
                hogged.set_volume(scalar)?;
                if let Some((current, _)) = hogged.read_volume() {
                    self.status.set_volume(current);
                } else {
                    self.status.set_volume(scalar);
                }
            }
            None => {
                // Sem device na mão, guarda para aplicar na aquisição. `value` é escalar de
                // 0 a 1 quando a unidade é porcentagem — não 0 a 100.
                inner.volume = Some(VolumeRequest {
                    valid: true,
                    unit: VolumeUnit::Percent,
                    value: scalar as f64,
                    reason: String::new(),
                });
                self.status.set_volume(scalar);
            }
        }
        Ok(())
    }

    /// A interface chama isto a cada poll: o fim da faixa é detectado pelo IOProc, que não
    /// pode mudar o estado do engine por estar em thread de tempo real.
    pub fn poll_finished(&self) {
        let mut inner = self.lock();
        if inner.state != PlayerState::Playing {
            return;
        }
        let terminou = inner
            .playback
            .as_ref()
            .is_some_and(|p| p.finished.load(std::sync::atomic::Ordering::Acquire));
        if terminou {
            if let Some(hogged) = inner.hogged.as_mut() {
                hogged.stop();
            }
            inner.state = PlayerState::Finished;
            self.status.set_state(PlayerState::Finished);
        }
    }

    pub fn shutdown(&self) -> Result<(), String> {
        let mut inner = self.lock();
        let restore = self.stop_and_release(&mut inner);
        self.teardown(&mut inner);
        inner.state = PlayerState::Idle;
        self.status.set_state(PlayerState::Idle);
        restore
    }
```

- [ ] **Step 5: Implementar a partida e a parada**

Ainda no `impl Engine`. O corpo de `start_from_beginning` é a sequência que hoje está em
`run()`, entre `let mut hogged = HoggedDevice::new();` e a criação da thread produtora — **na
mesma ordem**, porque cada passo dela corrigiu um defeito real:

```rust
    fn start_common(&self, inner: &mut EngineInner, attach_io_proc: bool) -> Result<(), String> {
        // Recarrega do início: vindo de Finished o decodificador já se esgotou, e mesmo
        // vindo de Loaded é preciso um AudioSource que possa ser movido para a produtora.
        let path = inner
            .path
            .clone()
            .ok_or_else(|| "nenhum arquivo carregado".to_string())?;
        let mut source = AudioSource::open(&path)?;
        let file_format = source.format();

        // Os dados que o resto da função precisa são copiados aqui para que os empréstimos
        // de `inner` terminem antes das atribuições no fim: sem isso o verificador de
        // empréstimos recusa a função.
        let device = inner
            .device
            .as_ref()
            .ok_or_else(|| "device não consultado".to_string())?;
        let decision = inner
            .decision
            .as_ref()
            .ok_or_else(|| "formato não negociado".to_string())?;

        let mut hogged = HoggedDevice::new();
        let physical = device.physical_formats[decision.physical_format_index as usize];
        hogged.acquire(device, decision.sample_rate, &physical)?;

        // Com o device já nosso e antes de qualquer amostra sair: é o único ponto em que dá
        // para garantir que o fone não receba o volume anterior.
        let outcome =
            apply_volume(&mut hogged, device, inner.volume.as_ref(), inner.ceiling)?;
        self.status.set_volume(outcome.scalar);

        let stream = hogged.stream_format();
        if stream.mFormatID != coreaudio_sys::kAudioFormatLinearPCM {
            return Err("o device não está em PCM linear; não vou alimentá-lo".to_string());
        }

        let (client, non_interleaved) = crate::engine::client_format_for(&stream);
        let check = crate::format::validate_interleaved_format(
            client.mBitsPerChannel,
            client.mBytesPerFrame,
            client.mChannelsPerFrame,
        );
        if !check.ok {
            return Err(format!("formato de entrega inconsistente: {}", check.reason));
        }

        source.set_client_format(&client)?;

        // O decodificador pode ajustar o que aceitou. Divergência aqui é a diferença entre
        // silêncio e ruído em volume total.
        let effective = source.effective_client_format()?;
        crate::engine::assert_same_delivery(&effective, &client)?;

        let ring_bytes = client.mSampleRate as usize * client.mBytesPerFrame as usize * 2;
        self.status.set_track(source.total_frames(), file_format.sample_rate);
        let playback = Arc::new(Playback::new(
            ring_bytes,
            &client,
            non_interleaved,
            Arc::clone(&self.status),
        ));

        let stop = Arc::new(AtomicBool::new(false));
        let producer = {
            let playback = Arc::clone(&playback);
            let stop = Arc::clone(&stop);
            let bytes_per_frame = client.mBytesPerFrame as usize;
            std::thread::spawn(move || {
                let mut chunk = vec![0u8; 64 * 1024];
                let frames_per_chunk = (chunk.len() / bytes_per_frame) as u32;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let got = source.read(&mut chunk, frames_per_chunk);
                    if got == 0 {
                        break;
                    }
                    let bytes = got as usize * bytes_per_frame;
                    let mut written = 0usize;
                    while written < bytes && !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        written += playback.ring.write(&chunk[written..bytes]);
                        if written < bytes {
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                    }
                }
                playback
                    .producer_done
                    .store(true, std::sync::atomic::Ordering::Release);
            })
        };

        // Deixa o buffer encher antes de abrir o fluxo, para o começo não sair picotado.
        let half = playback.ring.capacity() / 2;
        for _ in 0..200 {
            if playback.ring.available_to_read() >= half
                || playback
                    .producer_done
                    .load(std::sync::atomic::Ordering::Acquire)
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        // O modo dump usa exatamente este mesmo caminho, sem só esta última etapa: assim ele
        // percorre decodificador, ring buffer e alinhamento de frame, que é onde estiveram os
        // bugs mais caros. Um atalho que apenas decodificasse não provaria nada disso.
        if attach_io_proc {
            let context = Arc::as_ptr(&playback) as *mut std::ffi::c_void;
            hogged.start(Some(crate::playback::io_proc), context)?;
        }

        inner.volume_outcome = Some(outcome);
        inner.client = Some(client);
        inner.hogged = Some(hogged);
        inner.playback = Some(playback);
        inner.producer = Some(producer);
        inner.stop_producer = stop;
        Ok(())
    }

    /// Prepara tudo como o `play`, menos o IOProc, e devolve o que o consumidor de disco
    /// precisa. Usado pelo `--dump`.
    pub fn start_offline(
        &self,
    ) -> Result<(Arc<Playback>, AudioStreamBasicDescription), String> {
        let mut inner = self.lock();
        let target = next_state(inner.state, Command::Play).map_err(|e| e.message.to_string())?;
        self.start_common(&mut inner, false)?;
        let playback = inner
            .playback
            .clone()
            .ok_or_else(|| "playback não foi criado".to_string())?;
        let client = inner
            .client
            .ok_or_else(|| "formato de entrega não foi definido".to_string())?;
        inner.state = target;
        self.status.set_state(target);
        Ok((playback, client))
    }

    fn stop_and_release(&self, inner: &mut EngineInner) -> Result<(), String> {
        if let Some(hogged) = inner.hogged.as_mut() {
            hogged.stop();
        }
        inner
            .stop_producer
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(producer) = inner.producer.take() {
            let _ = producer.join();
        }
        inner.playback = None;
        let restore = match inner.hogged.as_mut() {
            Some(hogged) => hogged.finish(),
            None => Ok(()),
        };
        inner.hogged = None;
        restore
    }
```

E estenda o `teardown` da Task 5 para também soltar o device:

```rust
    fn teardown(&self, inner: &mut EngineInner) {
        let _ = self.stop_and_release(inner);
        inner.source = None;
        inner.decision = None;
        inner.device = None;
        inner.client = None;
        inner.path = None;
    }
```

- [ ] **Step 6: Mover as funções auxiliares de `main.rs` para `engine.rs`**

Mova de `rust/src/main.rs` para `rust/src/engine.rs`, tornando-as `pub(crate)`: `apply_volume` e
as duas abaixo, que hoje estão inline dentro de `run()`.

**Ao mover o `apply_volume`, remova dele todos os `print!`/`println!`.** Uma biblioteca usada
por um app gráfico não pode escrever em stdout. No lugar, faça-o devolver o que aconteceu, para
a CLI continuar imprimindo a mesma linha de hoje:

```rust
/// O que a aplicação de volume de fato fez, para quem quiser relatar.
pub struct VolumeOutcome {
    pub scalar: f32,
    pub decibels: f64,
    pub previous_scalar: f32, // negativo quando o device não expõe volume
    pub lowered_by_ceiling: bool,
}
```

`apply_volume` passa a ter assinatura
`pub(crate) fn apply_volume(hogged: &mut HoggedDevice, device: &OutputDevice, request:
Option<&VolumeRequest>, ceiling: f64) -> Result<VolumeOutcome, String>`, com a **mesma lógica de
decisão de hoje** — pedido explícito vence; sem pedido, o teto só age quando o volume atual o
ultrapassa. **Acrescente agora** o campo `volume_outcome: Option<VolumeOutcome>` ao
`EngineInner` e ao seu inicializador — ele não existe desde a Task 5 de propósito, porque o tipo
só nasce aqui. Guarde nele o resultado e exponha
`pub fn volume_outcome(&self) -> Option<VolumeOutcome>` clonando os campos, para a CLI imprimir.

```rust
/// O decodificador entrega sempre intercalado, que é como o ring buffer guarda; o IOProc
/// desintercala se o device pedir assim.
///
/// O tamanho do frame vem do próprio device, nunca de bitsPerChannel/8: um formato pode
/// carregar amostras de 24 bits em containers de 32, e presumir empacotamento faria o
/// decodificador produzir um passo e o IOProc ler outro — ruído branco, não música.
pub(crate) fn client_format_for(
    stream: &AudioStreamBasicDescription,
) -> (AudioStreamBasicDescription, bool) {
    let non_interleaved =
        stream.mFormatFlags & coreaudio_sys::kAudioFormatFlagIsNonInterleaved != 0;
    let mut client = *stream;
    client.mFormatFlags &= !coreaudio_sys::kAudioFormatFlagIsNonInterleaved;
    client.mFramesPerPacket = 1;
    client.mBytesPerFrame = if non_interleaved {
        stream.mBytesPerFrame * stream.mChannelsPerFrame
    } else {
        stream.mBytesPerFrame
    };
    client.mBytesPerPacket = client.mBytesPerFrame;
    (client, non_interleaved)
}

pub(crate) fn assert_same_delivery(
    effective: &AudioStreamBasicDescription,
    client: &AudioStreamBasicDescription,
) -> Result<(), String> {
    let float = coreaudio_sys::kAudioFormatFlagIsFloat;
    if effective.mBitsPerChannel != client.mBitsPerChannel
        || effective.mBytesPerFrame != client.mBytesPerFrame
        || effective.mChannelsPerFrame != client.mChannelsPerFrame
        || (effective.mFormatFlags & float) != (client.mFormatFlags & float)
        || (effective.mSampleRate - client.mSampleRate).abs() > 0.5
    {
        return Err(
            "o decodificador vai entregar um formato diferente do que o device espera; \
             reproduzir assim geraria ruído"
                .to_string(),
        );
    }
    Ok(())
}
```

- [ ] **Step 7: Reescrever o `run()` da CLI sobre o engine**

Em `rust/src/main.rs`, o `run()` passa a ser orquestração fina. Mantenha os handlers de sinal
onde estão — no topo, antes de qualquer coisa que altere o device — e mantenha as mensagens
impressas exatamente como estão hoje, porque a paridade com o C++ é verificada por elas.

```rust
fn run() -> i32 {
    for sig in [SIGINT, SIGTERM, SIGHUP, SIGQUIT] {
        unsafe { signal(sig, on_interrupt as usize) };
    }

    let options = match parse_args() {
        Ok(options) => options,
        Err(code) => return code,
    };

    let engine = Engine::new();
    engine.set_requested_volume(options.volume.clone(), options.ceiling);

    let track = match engine.load(&options.path) {
        Ok(track) => track,
        Err(error) => {
            eprintln!("erro: {error}");
            return 1;
        }
    };

    print_track_report(&options.path, &track);
    if options.info_only {
        return 0;
    }

    if let Some(path) = options.dump.as_ref() {
        return run_dump(&engine, path);
    }

    if let Err(error) = engine.play() {
        eprintln!("erro: {error}");
        return 1;
    }
    println!("tocando  : {:.1} s — Ctrl+C interrompe", track.total_seconds);

    while !interrupted() && engine.state() == PlayerState::Playing {
        std::thread::sleep(std::time::Duration::from_millis(50));
        engine.poll_finished();
    }

    let by_user = interrupted();
    let underruns = engine.status().underruns();
    if underruns > 0 {
        println!("aviso    : {underruns} falhas de alimentação do buffer");
    }

    if let Err(error) = engine.shutdown() {
        eprintln!(
            "aviso    : {error}\n\
             \x20          o device pode ter ficado com outra configuração; tocar\n\
             \x20          qualquer outro som ou abrir Configuração de Áudio e MIDI ajusta"
        );
        return 1;
    }

    println!("fim      : device restaurado");
    if by_user { 130 } else { 0 }
}
```

`print_track_report` substitui o antigo `print_device_report`:

```rust
fn print_track_report(path: &str, track: &LoadedTrack) {
    println!("arquivo  : {path}");
    println!(
        "fonte    : {} {} Hz / {} bits / {} canais / {:.1} s",
        track.codec, track.sample_rate, track.bit_depth, track.channels, track.total_seconds
    );
    println!("device   : {}", track.device_name);
}
```

A linha de volume continua sendo impressa pela CLI, depois do `play`, a partir de
`engine.volume_outcome()`.

O `run_dump` passa a obter o que consome pelo `start_offline`, e no fim encerra pelo engine.
Troque sua assinatura e as duas primeiras e últimas linhas; **o laço que grava em disco fica
exatamente como está**:

```rust
fn run_dump(engine: &Engine, path: &str) -> i32 {
    let (playback, client) = match engine.start_offline() {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("erro: {error}");
            return 1;
        }
    };

    // ... o laço existente de gravação, inalterado, usando `playback` e `client` ...

    if let Err(error) = engine.shutdown() {
        eprintln!("aviso    : {error}");
        return 1;
    }
    0
}
```

- [ ] **Step 8: Compilar e rodar toda a suíte**

```bash
cd rust && cargo build --release && cargo test 2>&1 | tail -6
```

Esperado: compila sem aviso; os testes sem hardware passam.

- [ ] **Step 9: Rodar os testes de hardware**

```bash
cd rust && cargo test -- --ignored --test-threads=1 2>&1 | tail -15
```

Esperado: todos passam, inclusive `ciclo_play_pause_play_shutdown`. Este é o teste que prova
que o tempo congela no pause e volta a andar no play.

- [ ] **Step 10: Rodar o gate de bit-perfect**

```bash
make verify FILE=testdata/t96_24.flac BITS=24
```

Esperado: três comparações bit-perfect. Se divergir, a reescrita alterou a ordem de operações
ou o formato de entrega — **pare e reveja o Step 5** antes de seguir.

- [ ] **Step 11: Verificar à mão que a CLI continua se comportando**

```bash
./rust/target/release/hog-audio --info testdata/t96_24.flac
./rust/target/release/hog-audio --volume 20 testdata/t44_16.flac
```

Esperado: o `--info` não toca no device; o segundo toca seis segundos de senoide e imprime
`fim      : device restaurado`. Interrompa outra execução com Ctrl+C e confirme que sai com
código 130 (`echo $?`) e restaura o device.

- [ ] **Step 12: Commit**

```bash
git add rust/src/device.rs rust/src/engine.rs rust/src/main.rs
git commit -m "feat(engine): adicionar play, pause, volume e encerramento"
```

---

### Task 7: As duas provas novas de bit-perfect

O pause introduz um risco concreto: se alguém um dia "otimizar" a pausa esvaziando o ring
buffer ou reiniciando índices, o áudio pula um trecho — e nada no código acusa. Esta task cria
a prova que reprova essa mudança.

A segunda prova protege contra volume implementado em software.

**Files:**
- Modify: `rust/src/main.rs`, `Makefile`
- Test: alvo `make verify-pause` e alvo `make verify-volume`

**Interfaces:**
- Consumes: `Engine::start_offline` da Task 6.
- Produces: a opção de linha de comando `--pause-at FRAMES`, e os alvos `verify-pause` e `verify-volume` no `Makefile`.

- [ ] **Step 1: Acrescentar `--pause-at` ao parser de argumentos**

Em `rust/src/main.rs`, acrescente o campo à `struct Options`:

```rust
    pause_at: Option<usize>,
```

inicialize com `pause_at: None` em `parse_args`, e acrescente o braço ao `match`:

```rust
            "--pause-at" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("erro: --pause-at exige um número de frames");
                    return Err(2);
                };
                match value.parse::<usize>() {
                    Ok(frames) => options.pause_at = Some(frames),
                    Err(_) => {
                        eprintln!("erro: --pause-at espera um número inteiro de frames");
                        return Err(2);
                    }
                }
            }
```

Acrescente a linha correspondente ao texto de `usage()`:

```
         \x20 --pause-at N    no modo dump, simula uma pausa depois de N frames\n\
```

- [ ] **Step 2: Simular a pausa no laço de dump**

No `run_dump`, dentro do laço de gravação, logo após o contador de frames escritos ser
atualizado, insira:

```rust
        // Reproduz o que o pause de verdade faz: o consumidor simplesmente deixa de ser
        // chamado por um tempo. O ring buffer não é tocado, a produtora enche e bloqueia, e
        // ao voltar a leitura continua no byte seguinte. Se algum dia a pausa passar a
        // descartar ou reiniciar o buffer, a saída deixa de bater e este teste reprova.
        if let Some(limite) = pause_at {
            if !ja_pausou && frames_written >= limite {
                ja_pausou = true;
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
        }
```

Declare antes do laço `let mut ja_pausou = false;` e passe `pause_at: Option<usize>` como
parâmetro de `run_dump`.

- [ ] **Step 3: Compilar e gerar os dois dumps**

```bash
cd rust && cargo build --release && cd ..
./rust/target/release/hog-audio --dump /tmp/sem_pausa.raw testdata/t96_24.flac
./rust/target/release/hog-audio --dump /tmp/com_pausa.raw --pause-at 100000 testdata/t96_24.flac
ls -l /tmp/sem_pausa.raw /tmp/com_pausa.raw
```

Esperado: os dois arquivos existem, com tamanho idêntico e diferente de zero (~4,6 MB).

- [ ] **Step 4: Comparar**

```bash
cmp /tmp/sem_pausa.raw /tmp/com_pausa.raw && echo "IDENTICOS: a pausa nao alterou o fluxo"
```

Esperado: idênticos.

- [ ] **Step 5: Provar que esta comparação consegue reprovar**

Um teste que nunca falhou não provou nada. Troque temporariamente, dentro do bloco da pausa,
`std::thread::sleep(...)` por uma leitura destrutiva que descarta um bloco do ring:

```rust
                let mut lixo = vec![0u8; 4096];
                playback.ring.read(&mut lixo);
```

Recompile, gere `/tmp/com_pausa.raw` de novo e rode o `cmp`.

Esperado: **DIVERGEM**. Isso confirma que a comparação detecta perda de bytes na pausa. Desfaça
a mutação, recompile e confirme que volta a bater.

- [ ] **Step 6: Acrescentar os dois alvos ao `Makefile`**

```makefile
# make verify-pause FILE=testdata/t96_24.flac
# O pause nao pode descartar nem duplicar bytes do ring buffer: o dump com uma pausa
# injetada no meio tem de sair identico ao dump sem pausa.
verify-pause: rust
	@test -n "$(FILE)" || { echo "uso: make verify-pause FILE=arquivo.flac"; exit 2; }
	@./$(RUST_BIN) --dump /tmp/hog_sem_pausa.raw "$(FILE)" >/dev/null
	@./$(RUST_BIN) --dump /tmp/hog_com_pausa.raw --pause-at 100000 "$(FILE)" >/dev/null
	@test -s /tmp/hog_sem_pausa.raw || { echo "FALHOU: dump vazio"; exit 1; }
	@cmp /tmp/hog_sem_pausa.raw /tmp/hog_com_pausa.raw \
	  && echo "pause: fluxo identico com e sem pausa"

# make verify-volume FILE=testdata/t96_24.flac
# O volume e aplicado no device, nunca nas amostras. O modo dump adquire o device e aplica
# o volume de verdade, entao este teste reprova se alguem implementar ganho em software.
verify-volume: rust
	@test -n "$(FILE)" || { echo "uso: make verify-volume FILE=arquivo.flac"; exit 2; }
	@./$(RUST_BIN) --dump /tmp/hog_vol20.raw --volume 20 "$(FILE)" >/dev/null
	@./$(RUST_BIN) --dump /tmp/hog_vol90.raw --volume 90 "$(FILE)" >/dev/null
	@test -s /tmp/hog_vol20.raw || { echo "FALHOU: dump vazio"; exit 1; }
	@cmp /tmp/hog_vol20.raw /tmp/hog_vol90.raw \
	  && echo "volume: amostras identicas a 20% e 90%"
```

Acrescente `verify-pause verify-volume` à linha `.PHONY`.

O `test -s` não é decoração: dois arquivos vazios também passariam no `cmp`, e o teste
deixaria de poder reprovar.

- [ ] **Step 7: Rodar os três gates**

```bash
make verify FILE=testdata/t96_24.flac BITS=24
make verify-pause FILE=testdata/t96_24.flac
make verify-volume FILE=testdata/t96_24.flac
```

Esperado: bit-perfect nas três comparações, fluxo idêntico com e sem pausa, amostras idênticas
nos dois volumes.

- [ ] **Step 8: Commit**

```bash
git add rust/src/main.rs Makefile
git commit -m "test(bitperfect): provar continuidade da pausa e independencia do volume"
```

---

### Task 8: Superfície uniffi e geração dos bindings

**Files:**
- Create: `rust/src/api.rs`, `rust/src/bin/uniffi-bindgen.rs`
- Modify: `rust/Cargo.toml`, `rust/src/lib.rs`, `rust/src/transitions.rs`, `Makefile`

**Interfaces:**
- Consumes: `Engine`, `LoadedTrack` da Task 6; `PlayerState` de `transitions.rs`.
- Produces: os tipos uniffi `HogPlayer`, `Snapshot`, `TrackFormat`, `PlayerError`; e os arquivos gerados `hog_audio.swift`, `hog_audioFFI.h`, `hog_audioFFI.modulemap`.

Toda esta cadeia foi verificada num spike antes deste plano existir: a API de proc-macro
compila, o `uniffi-bindgen` lê a **staticlib** (`.a`) e produz saída idêntica à gerada a partir
de uma dylib, e o Swift recebe enum tipado e `Result::Err` como `throw` tipado.

- [ ] **Step 1: Acrescentar as dependências e o binário gerador**

Em `rust/Cargo.toml`:

```toml
[[bin]]
name = "uniffi-bindgen"
path = "src/bin/uniffi-bindgen.rs"

[dependencies]
coreaudio-sys = "0.2.18"
uniffi = { version = "0.32", features = ["cli"] }
```

O `features = ["cli"]` é o que expõe `uniffi_bindgen_main`. Sem ele o binário não compila.

Crie `rust/src/bin/uniffi-bindgen.rs`:

```rust
fn main() {
    uniffi::uniffi_bindgen_main()
}
```

- [ ] **Step 2: Marcar `PlayerState` como enum do uniffi**

Em `rust/src/transitions.rs`, acrescente o derive:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PlayerState {
```

- [ ] **Step 3: Escrever `rust/src/api.rs`**

```rust
//! A superfície que o Swift enxerga. Só controle e leitura de estado atravessam esta
//! fronteira: nenhum dado de áudio passa por aqui, e o IOProc nunca a alcança.

use std::sync::Arc;

use crate::engine::Engine;
use crate::transitions::PlayerState;
use crate::volume::{parse_volume, VolumeRequest, VolumeUnit};

#[derive(uniffi::Record)]
pub struct Snapshot {
    pub state: PlayerState,
    pub elapsed_seconds: f64,
    pub total_seconds: f64,
    pub underruns: u64,
    pub volume_scalar: f32,
}

#[derive(uniffi::Record)]
pub struct TrackFormat {
    pub sample_rate: f64,
    pub bit_depth: u32,
    pub channels: u32,
    pub codec: String,
    pub device_name: String,
    pub total_seconds: f64,
}

#[derive(uniffi::Error, Debug)]
pub enum PlayerError {
    Load { message: String },
    Device { message: String },
    State { message: String },
}

impl std::fmt::Display for PlayerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayerError::Load { message }
            | PlayerError::Device { message }
            | PlayerError::State { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for PlayerError {}

#[derive(uniffi::Object)]
pub struct HogPlayer {
    engine: Engine,
}

#[uniffi::export]
impl HogPlayer {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            engine: Engine::new(),
        })
    }

    pub fn load(&self, path: String) -> Result<TrackFormat, PlayerError> {
        let track = self
            .engine
            .load(&path)
            .map_err(|message| PlayerError::Load { message })?;
        Ok(TrackFormat {
            sample_rate: track.sample_rate,
            bit_depth: track.bit_depth,
            channels: track.channels,
            codec: track.codec,
            device_name: track.device_name,
            total_seconds: track.total_seconds,
        })
    }

    pub fn play(&self) -> Result<(), PlayerError> {
        self.engine
            .play()
            .map_err(|message| PlayerError::Device { message })
    }

    pub fn pause(&self) -> Result<(), PlayerError> {
        self.engine
            .pause()
            .map_err(|message| PlayerError::State { message })
    }

    pub fn set_volume(&self, scalar: f32) -> Result<(), PlayerError> {
        let scalar = scalar.clamp(0.0, 1.0);
        self.engine
            .set_volume(scalar)
            .map_err(|message| PlayerError::Device { message })
    }

    /// Só lê atômicos, então nunca bloqueia — mesmo com um `play` lento segurando o mutex do
    /// engine na aquisição do hog. É o que permite a interface consultar dez vezes por
    /// segundo sem travar.
    pub fn snapshot(&self) -> Snapshot {
        self.engine.poll_finished();
        let status = self.engine.status();
        Snapshot {
            state: status.state(),
            elapsed_seconds: status.elapsed_seconds(),
            total_seconds: status.total_seconds(),
            underruns: status.underruns(),
            volume_scalar: status.volume(),
        }
    }

    pub fn shutdown(&self) {
        let _ = self.engine.shutdown();
    }
}

/// Interpreta `35`, `35%` ou `-18dB` do mesmo jeito que a linha de comando, para a interface
/// não precisar reimplementar a regra.
#[uniffi::export]
pub fn parse_volume_text(text: String) -> Option<f32> {
    let parsed: VolumeRequest = parse_volume(&text);
    if !parsed.valid || parsed.unit != VolumeUnit::Percent {
        return None;
    }
    // `value` já vem como escalar de 0 a 1; dividir por 100 aqui daria 0,0035 para "35".
    Some(parsed.value as f32)
}
```

O `poll_finished` fica dentro do `snapshot` porque o fim da faixa é detectado pelo IOProc, que
está em thread de tempo real e não pode mudar o estado do engine; alguém precisa converter esse
sinal em transição, e o poll da interface é o lugar natural.

- [ ] **Step 4: Declarar o módulo e o scaffolding**

Em `rust/src/lib.rs`, acrescente ao topo (antes das declarações de módulo):

```rust
uniffi::setup_scaffolding!();
```

e `pub mod api;` na lista de módulos, em ordem alfabética.

- [ ] **Step 5: Compilar e verificar que o `Engine` atravessa a fronteira**

```bash
cd rust && cargo build --release 2>&1 | tail -20
```

Esperado: compila. Se aparecer `HogPlayer cannot be shared between threads safely`, é porque
algum campo do `EngineInner` não é `Send`. Nesse caso acrescente em `engine.rs`, com a
justificativa escrita:

```rust
// O EngineInner guarda identificadores opacos do Core Audio, que o Rust trata como
// não-enviáveis por precaução. Mantê-lo atrás de um Mutex garante que só uma thread o toque
// por vez, que é exatamente a condição que torna o acesso seguro.
unsafe impl Send for EngineInner {}
```

- [ ] **Step 6: Gerar os bindings Swift**

```bash
cd rust && cargo run --release --bin uniffi-bindgen -- \
  generate --library target/release/libhog_audio.a \
  --language swift --out-dir /tmp/hog_bindings
find /tmp/hog_bindings -type f
```

Esperado: três arquivos — `hog_audio.swift`, `hog_audioFFI.h`, `hog_audioFFI.modulemap`.

- [ ] **Step 7: Acrescentar os alvos ao `Makefile`**

Os arquivos gerados vao direto para os alvos do SPM que a Task 9 cria: o header e o modulemap
para um alvo C, e o Swift para um alvo Swift. O modulemap precisa se chamar `module.modulemap`
para o SPM reconhece-lo.

```makefile
RUST_LIB  := rust/target/release/libhog_audio.a
FFI_DIR   := apps/player/Sources/HogAudioFFI
BIND_DIR  := apps/player/Sources/HogAudioBindings

rust-lib:
	@cd rust && cargo build --release

# Os bindings sao gerados a partir da staticlib, entao o .app nao precisa embarcar dylib.
bindings: rust-lib
	@mkdir -p $(FFI_DIR)/include $(BIND_DIR)
	@cd rust && cargo run -q --release --bin uniffi-bindgen -- \
	  generate --library target/release/libhog_audio.a \
	  --language swift --out-dir /tmp/hog_bindings
	@cp /tmp/hog_bindings/hog_audioFFI.h $(FFI_DIR)/include/
	@cp /tmp/hog_bindings/hog_audioFFI.modulemap $(FFI_DIR)/include/module.modulemap
	@cp /tmp/hog_bindings/hog_audio.swift $(BIND_DIR)/
	@echo '// um alvo C do SPM exige ao menos um arquivo-fonte' > $(FFI_DIR)/empty.c
	@echo "bindings gerados em $(FFI_DIR) e $(BIND_DIR)"
```

Acrescente `rust-lib bindings` à linha `.PHONY`.

- [ ] **Step 8: Ignorar os arquivos gerados no git**

Acrescente ao `.gitignore`:

```
apps/player/Sources/HogAudioFFI/
apps/player/Sources/HogAudioBindings/
apps/player/.build/
apps/player/HogAudio.app/
```

São produtos de build, reconstruídos por `make bindings`. Versioná-los criaria divergência
silenciosa entre o Rust e o Swift.

- [ ] **Step 9: Confirmar que os gates continuam de pé**

```bash
cd rust && cargo test 2>&1 | tail -5
cd .. && make verify FILE=testdata/t96_24.flac BITS=24
```

Esperado: testes passando e bit-perfect. Acrescentar a fronteira não pode ter mudado o áudio.

- [ ] **Step 10: Commit**

```bash
git add rust/Cargo.toml rust/Cargo.lock rust/src/api.rs rust/src/bin/uniffi-bindgen.rs \
        rust/src/lib.rs rust/src/transitions.rs Makefile .gitignore
git commit -m "feat(api): expor o engine ao swift via uniffi"
```

---

### Task 9: Pacote Swift e a ponte de verdade

Prova que o Swift chama o Rust e recebe o estado correto. A linkagem foi verificada num spike
antes deste plano: `-L` com caminho **relativo** funciona, e o `.a` linka sem dylib.

**Files:**
- Create: `apps/player/Package.swift`, `apps/player/Tests/HogPlayerKitTests/BridgeTests.swift`, `apps/player/Sources/HogPlayerKit/Placeholder.swift`
- Modify: `Makefile`

**Interfaces:**
- Consumes: os bindings gerados na Task 8 — `HogPlayer`, `Snapshot`, `TrackFormat`, `PlayerError`, `PlayerState`, `parseVolumeText`.
- Produces: os alvos SPM `HogAudioFFI`, `HogAudioBindings`, `HogPlayerKit`, `HogPlayer`; e o alvo `make swift-test`.

- [ ] **Step 1: Gerar os bindings antes de qualquer coisa**

```bash
make bindings
find apps/player/Sources -type f | sort
```

Esperado: `HogAudioFFI/empty.c`, `HogAudioFFI/include/hog_audioFFI.h`,
`HogAudioFFI/include/module.modulemap`, `HogAudioBindings/hog_audio.swift`.

- [ ] **Step 2: Escrever o `Package.swift`**

Os frameworks precisam ser declarados aqui explicitamente. O `build.rs` do Rust emite
`cargo:rustc-link-lib=framework=...`, mas isso só vale quando **o cargo** faz a linkagem; ao
linkar a staticlib com o `swiftc`, essas diretivas não se propagam e o resultado seria uma
enxurrada de símbolos indefinidos do Core Audio.

```swift
// swift-tools-version:6.0
import PackageDescription

// Caminho relativo ao diretório do pacote: verificado que o SPM resolve corretamente.
let rustLib = "../../rust/target/release"

let linkRust: [LinkerSetting] = [
    .unsafeFlags(["-L\(rustLib)", "-lhog_audio"]),
    .linkedFramework("CoreAudio"),
    .linkedFramework("AudioToolbox"),
    .linkedFramework("CoreFoundation"),
]

let package = Package(
    name: "HogPlayer",
    platforms: [.macOS(.v14)],
    targets: [
        .target(name: "HogAudioFFI"),
        .target(name: "HogAudioBindings", dependencies: ["HogAudioFFI"]),
        .target(name: "HogPlayerKit", dependencies: ["HogAudioBindings"]),
        .executableTarget(
            name: "HogPlayer",
            dependencies: ["HogPlayerKit"],
            linkerSettings: linkRust
        ),
        .testTarget(
            name: "HogPlayerKitTests",
            dependencies: ["HogPlayerKit"],
            linkerSettings: linkRust
        ),
    ]
)
```

- [ ] **Step 3: Criar um arquivo mínimo no `HogPlayerKit`**

Um alvo Swift sem nenhum fonte não compila. Crie
`apps/player/Sources/HogPlayerKit/Placeholder.swift`:

```swift
import HogAudioBindings

/// Reexporta o player para quem depende só do Kit.
public typealias Player = HogPlayer
```

- [ ] **Step 4: Escrever o teste da ponte**

Crie `apps/player/Tests/HogPlayerKitTests/BridgeTests.swift`:

```swift
import Testing
@testable import HogPlayerKit
import HogAudioBindings

@Test func playerNovoComecaEmIdle() {
    let player = HogPlayer()
    let snapshot = player.snapshot()
    #expect(snapshot.state == .idle)
    #expect(snapshot.elapsedSeconds == 0)
    #expect(snapshot.underruns == 0)
}

@Test func arquivoInexistenteLancaErroTipado() {
    let player = HogPlayer()
    #expect(throws: PlayerError.self) {
        _ = try player.load(path: "/tmp/nao-existe-mesmo-12345.flac")
    }
}

@Test func aInterpretacaoDeVolumeAtravessaAFronteira() {
    // A regra vive no Rust; a interface não pode ter uma segunda cópia dela.
    #expect(parseVolumeText(text: "35") == 0.35)
    #expect(parseVolumeText(text: "35%") == 0.35)
    #expect(parseVolumeText(text: "35x") == nil)
}
```

- [ ] **Step 5: Rodar os testes e vê-los falhar**

```bash
cd apps/player && swift test 2>&1 | tail -20
```

Esperado nesta primeira execução: **falha de compilação** se algum nome não bater — o uniffi
converte `snake_case` do Rust em `camelCase` no Swift, então `elapsed_seconds` vira
`elapsedSeconds` e `parse_volume_text` vira `parseVolumeText`. Se falhar, abra
`apps/player/Sources/HogAudioBindings/hog_audio.swift` e confira os nomes reais gerados,
ajustando o teste. É o gerado que manda.

- [ ] **Step 6: Rodar os testes e vê-los passar**

```bash
cd apps/player && swift test 2>&1 | tail -12
```

Esperado: 3 testes passando. Este é o momento em que a ponte Rust↔Swift está provada de ponta
a ponta.

- [ ] **Step 7: Acrescentar o alvo ao `Makefile`**

```makefile
swift-test: bindings
	@cd apps/player && swift test
```

Acrescente `swift-test` à linha `.PHONY` e ao alvo agregado:

```makefile
test: cpp-test rust-test swift-test
```

- [ ] **Step 8: Commit**

```bash
git add apps/player/Package.swift apps/player/Sources/HogPlayerKit/Placeholder.swift \
        apps/player/Tests Makefile
git commit -m "feat(app): adicionar pacote swift linkando o engine rust"
```

---

### Task 10: Metadados da faixa

A leitura das tags fica em Swift porque o AVFoundation já resolve tudo — inclusive desmontar o
bloco de imagem do FLAC e devolver o JPEG pronto. Isto foi medido nos arquivos reais do
projeto, e a tabela abaixo é o resultado da medição, não suposição:

| Formato | `commonMetadata` | Keyspace cru | Título | Artista | Álbum | Capa |
|---|---|---|---|---|---|---|
| FLAC, Ogg | **vazio** | `vorb` | `TITLE` | `ARTIST` | `ALBUM` | `METADATA_BLOCK_PICTURE` |
| M4A, ALAC, AAC | funciona | `itsk` | — | — | — | — |
| MP3 | funciona | `org.id3` | — | — | — | — |

Só o FLAC precisa do caminho alternativo, e é justamente o formato principal do projeto.

**Files:**
- Create: `apps/player/Sources/HogPlayerKit/Metadata.swift`, `apps/player/Tests/HogPlayerKitTests/MetadataTests.swift`

**Interfaces:**
- Consumes: nada do Rust.
- Produces: `TrackMetadata { title: String, artist: String?, album: String?, artwork: Data? }`, `MetadataItem { keySpace: String, key: String, stringValue: String?, dataValue: Data? }`, `func trackMetadata(common: [MetadataItem], raw: [MetadataItem], fallbackFilename: String) -> TrackMetadata`, e `func loadMetadata(from url: URL) async -> TrackMetadata`.

- [ ] **Step 1: Escrever os testes que falham**

Crie `apps/player/Tests/HogPlayerKitTests/MetadataTests.swift`:

```swift
import Testing
import Foundation
@testable import HogPlayerKit

private func item(_ keySpace: String, _ key: String, _ value: String) -> MetadataItem {
    MetadataItem(keySpace: keySpace, key: key, stringValue: value, dataValue: nil)
}

@Test func usaCommonMetadataQuandoDisponivel() {
    let common = [
        item("", "title", "Skyfall"),
        item("", "artist", "Adele"),
        item("", "albumName", "Skyfall"),
    ]
    let meta = trackMetadata(common: common, raw: [], fallbackFilename: "ignorado")
    #expect(meta.title == "Skyfall")
    #expect(meta.artist == "Adele")
    #expect(meta.album == "Skyfall")
}

@Test func caiNoVorbisQuandoCommonVemVazio() {
    // É exatamente o caso do FLAC: commonMetadata devolve lista vazia, medido.
    let raw = [
        item("vorb", "TITLE", "House of Memories"),
        item("vorb", "ARTIST", "Panic! At The Disco"),
        item("vorb", "ALBUM", "Death of a Bachelor"),
        item("vorb", "TRACKNUMBER", "10"),
    ]
    let meta = trackMetadata(common: [], raw: raw, fallbackFilename: "ignorado")
    #expect(meta.title == "House of Memories")
    #expect(meta.artist == "Panic! At The Disco")
    #expect(meta.album == "Death of a Bachelor")
}

@Test func leTagsDeId3() {
    let raw = [
        item("org.id3", "TIT2", "Titulo Teste"),
        item("org.id3", "TPE1", "Artista Teste"),
        item("org.id3", "TALB", "Album Teste"),
    ]
    let meta = trackMetadata(common: [], raw: raw, fallbackFilename: "ignorado")
    #expect(meta.title == "Titulo Teste")
    #expect(meta.artist == "Artista Teste")
    #expect(meta.album == "Album Teste")
}

@Test func semTagsUsaONomeDoArquivo() {
    let meta = trackMetadata(common: [], raw: [], fallbackFilename: "10-House-of-Memories")
    #expect(meta.title == "10-House-of-Memories")
    #expect(meta.artist == nil)
    #expect(meta.album == nil)
}

@Test func tagVaziaNaoVenceONomeDoArquivo() {
    let raw = [item("vorb", "TITLE", "   ")]
    let meta = trackMetadata(common: [], raw: raw, fallbackFilename: "faixa")
    #expect(meta.title == "faixa")
}

@Test func aCapaVemDoMetadataBlockPicture() {
    let jpeg = Data([0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10])
    let raw = [
        MetadataItem(keySpace: "vorb", key: "METADATA_BLOCK_PICTURE",
                     stringValue: nil, dataValue: jpeg)
    ]
    let meta = trackMetadata(common: [], raw: raw, fallbackFilename: "faixa")
    #expect(meta.artwork == jpeg)
}
```

- [ ] **Step 2: Rodar e ver falhar**

```bash
cd apps/player && swift test --filter MetadataTests 2>&1 | tail -15
```

Esperado: erro de compilação — `cannot find MetadataItem in scope`.

- [ ] **Step 3: Implementar**

Crie `apps/player/Sources/HogPlayerKit/Metadata.swift`:

```swift
import AVFoundation
import Foundation

public struct TrackMetadata: Equatable, Sendable {
    public let title: String
    public let artist: String?
    public let album: String?
    public let artwork: Data?
}

/// Um item de metadado já extraído do AVFoundation. A separação existe para que a regra de
/// mapeamento seja testável sem arquivo nenhum.
public struct MetadataItem: Sendable {
    public let keySpace: String
    public let key: String
    public let stringValue: String?
    public let dataValue: Data?

    public init(keySpace: String, key: String, stringValue: String?, dataValue: Data?) {
        self.keySpace = keySpace
        self.key = key
        self.stringValue = stringValue
        self.dataValue = dataValue
    }
}

private let titleKeys: Set<String> = ["title", "TITLE", "TIT2"]
private let artistKeys: Set<String> = ["artist", "ARTIST", "TPE1"]
private let albumKeys: Set<String> = ["albumName", "ALBUM", "TALB"]
private let artworkKeys: Set<String> = ["artwork", "METADATA_BLOCK_PICTURE", "APIC"]

private func firstText(_ items: [MetadataItem], _ keys: Set<String>) -> String? {
    for item in items where keys.contains(item.key) {
        if let value = item.stringValue?.trimmingCharacters(in: .whitespacesAndNewlines),
           !value.isEmpty {
            return value
        }
    }
    return nil
}

private func firstData(_ items: [MetadataItem], _ keys: Set<String>) -> Data? {
    for item in items where keys.contains(item.key) {
        if let data = item.dataValue, !data.isEmpty { return data }
    }
    return nil
}

/// O FLAC devolve `commonMetadata` vazio — medido —, então a lista crua é o caminho
/// alternativo. Nos demais formatos o comum já traz tudo, capa inclusa.
public func trackMetadata(
    common: [MetadataItem],
    raw: [MetadataItem],
    fallbackFilename: String
) -> TrackMetadata {
    let todos = common + raw
    return TrackMetadata(
        title: firstText(todos, titleKeys) ?? fallbackFilename,
        artist: firstText(todos, artistKeys),
        album: firstText(todos, albumKeys),
        artwork: firstData(todos, artworkKeys)
    )
}

/// Lê os metadados de um arquivo. Nunca falha: sem tags legíveis, o título vira o nome do
/// arquivo.
public func loadMetadata(from url: URL) async -> TrackMetadata {
    let asset = AVURLAsset(url: url)
    let nome = url.deletingPathExtension().lastPathComponent

    func converter(_ items: [AVMetadataItem]) async -> [MetadataItem] {
        var resultado: [MetadataItem] = []
        for item in items {
            let chave = item.commonKey?.rawValue
                ?? item.key.map { "\($0)" }
                ?? item.identifier?.rawValue
                ?? ""
            let texto = try? await item.load(.stringValue)
            let dados = try? await item.load(.dataValue)
            resultado.append(MetadataItem(
                keySpace: item.keySpace?.rawValue ?? "",
                key: chave,
                stringValue: texto ?? nil,
                dataValue: dados ?? nil
            ))
        }
        return resultado
    }

    guard let common = try? await asset.load(.commonMetadata),
          let raw = try? await asset.load(.metadata) else {
        return TrackMetadata(title: nome, artist: nil, album: nil, artwork: nil)
    }

    return trackMetadata(
        common: await converter(common),
        raw: await converter(raw),
        fallbackFilename: nome
    )
}
```

- [ ] **Step 4: Rodar e ver passar**

```bash
cd apps/player && swift test --filter MetadataTests 2>&1 | tail -12
```

Esperado: 6 testes passando.

- [ ] **Step 5: Verificar contra os arquivos reais**

Os testes acima usam dados sintéticos, e dado sintético confirma o mapeamento mas não confirma
que o AVFoundation entrega o que supomos. Acrescente ao final de `MetadataTests.swift`:

```swift
@Test func leUmFlacRealComCapa() async {
    // Caminho relativo a apps/player, que é de onde `swift test` roda.
    let url = URL(fileURLWithPath: "../../musicas/Skyfall.flac")
    guard FileManager.default.fileExists(atPath: url.path) else {
        return // o arquivo não é versionado; sem ele não há o que verificar
    }
    let meta = await loadMetadata(from: url)
    #expect(meta.title == "Skyfall")
    #expect(meta.artist == "Adele")
    // O AVFoundation desmonta o bloco de imagem do FLAC e entrega JPEG puro: FF D8 é o
    // marcador de início. Verificado neste arquivo, 38.583 bytes.
    #expect(meta.artwork != nil)
    #expect(meta.artwork?.prefix(2).elementsEqual([0xFF, 0xD8]) == true)
}

@Test func leUmFlacRealSemCapa() async {
    let url = URL(fileURLWithPath: "../../musicas/10-House-of-Memories.flac")
    guard FileManager.default.fileExists(atPath: url.path) else { return }
    let meta = await loadMetadata(from: url)
    #expect(meta.title == "House of Memories")
    #expect(meta.artist == "Panic! At The Disco")
    #expect(meta.album == "Death of a Bachelor")
    #expect(meta.artwork == nil) // este arquivo não tem capa embutida, verificado com ffprobe
}
```

```bash
cd apps/player && swift test --filter MetadataTests 2>&1 | tail -12
```

Esperado: 8 testes passando.

- [ ] **Step 6: Commit**

```bash
git add apps/player/Sources/HogPlayerKit/Metadata.swift \
        apps/player/Tests/HogPlayerKitTests/MetadataTests.swift
git commit -m "feat(app): ler titulo, artista, album e capa da faixa"
```

---

### Task 11: Formatação de tempo e tradução do snapshot

Duas funções puras: o que aparece no relógio e o que aparece no resto da tela. Separá-las do
timer é o que as torna testáveis sem hardware e sem esperar segundos passarem.

**Files:**
- Create: `apps/player/Sources/HogPlayerKit/TimeFormat.swift`, `apps/player/Sources/HogPlayerKit/DisplayState.swift`, `apps/player/Tests/HogPlayerKitTests/DisplayTests.swift`

**Interfaces:**
- Consumes: `Snapshot`, `PlayerState`, `TrackFormat` dos bindings; `TrackMetadata` da Task 10.
- Produces: `func formatTime(_ seconds: Double) -> String`; `DisplayState { title, artist, album, artwork, elapsed, total, progress, canPlay, canPause, isPlaying, technicalLine, message }`; `func displayState(snapshot:metadata:format:) -> DisplayState`.

- [ ] **Step 1: Escrever os testes que falham**

Crie `apps/player/Tests/HogPlayerKitTests/DisplayTests.swift`:

```swift
import Testing
import Foundation
@testable import HogPlayerKit
import HogAudioBindings

@Test func formataSegundosComoRelogio() {
    #expect(formatTime(0) == "0:00")
    #expect(formatTime(7) == "0:07")
    #expect(formatTime(83) == "1:23")
    #expect(formatTime(3599) == "59:59")
    #expect(formatTime(3600) == "1:00:00")
    #expect(formatTime(3723) == "1:02:03")
}

@Test func tempoInvalidoNaoQuebraORelogio() {
    // total_seconds vem zerado antes de qualquer carga, e NaN é o que sai de uma divisão
    // por taxa zero. Nenhum dos dois pode virar "-1:-1" na tela.
    #expect(formatTime(-5) == "0:00")
    #expect(formatTime(.nan) == "0:00")
    #expect(formatTime(.infinity) == "0:00")
}

private let faixa = TrackFormat(
    sampleRate: 96000, bitDepth: 24, channels: 2,
    codec: "flac", deviceName: "Fones de Ouvido Externos", totalSeconds: 208.7
)

private let tags = TrackMetadata(
    title: "House of Memories", artist: "Panic! At The Disco",
    album: "Death of a Bachelor", artwork: nil
)

private func snap(_ state: PlayerState, elapsed: Double = 0, underruns: UInt64 = 0) -> Snapshot {
    Snapshot(state: state, elapsedSeconds: elapsed, totalSeconds: 208.7,
             underruns: underruns, volumeScalar: 0.5)
}

@Test func tocandoMostraPausarEOProgresso() {
    let d = displayState(snapshot: snap(.playing, elapsed: 83), metadata: tags, format: faixa)
    #expect(d.isPlaying)
    #expect(d.canPause)
    #expect(d.elapsed == "1:23")
    #expect(d.total == "3:28")
    #expect(abs(d.progress - 83 / 208.7) < 0.001)
    #expect(d.title == "House of Memories")
}

@Test func semFaixaCarregadaNaoDaParaTocar() {
    let vazio = Snapshot(state: .idle, elapsedSeconds: 0, totalSeconds: 0,
                         underruns: 0, volumeScalar: 0.5)
    let d = displayState(snapshot: vazio, metadata: nil, format: nil)
    #expect(!d.canPlay)
    #expect(!d.canPause)
    #expect(d.progress == 0)
    #expect(d.technicalLine.isEmpty)
}

@Test func aLinhaTecnicaMostraOQueOProjetoFazDeDiferente() {
    let d = displayState(snapshot: snap(.playing), metadata: tags, format: faixa)
    #expect(d.technicalLine.contains("96 kHz"))
    #expect(d.technicalLine.contains("24 bit"))
    #expect(d.technicalLine.contains("flac"))
    #expect(d.technicalLine.contains("hog ativo"))
}

@Test func pausadoAindaSegurraODevice() {
    let d = displayState(snapshot: snap(.paused, elapsed: 40), metadata: tags, format: faixa)
    #expect(!d.isPlaying)
    #expect(d.canPlay)
    #expect(d.technicalLine.contains("hog ativo"))
}

@Test func carregadoAindaNaoSegurraODevice() {
    let d = displayState(snapshot: snap(.loaded), metadata: tags, format: faixa)
    #expect(d.canPlay)
    #expect(!d.technicalLine.contains("hog ativo"))
}

@Test func underrunsAparecemNaLinhaTecnica() {
    let d = displayState(snapshot: snap(.playing, underruns: 3), metadata: tags, format: faixa)
    #expect(d.technicalLine.contains("3 underruns"))
}

@Test func aFaixaTerminadaPodeSerTocadaDeNovo() {
    let d = displayState(snapshot: snap(.finished, elapsed: 208.7), metadata: tags, format: faixa)
    #expect(d.canPlay)
    #expect(!d.canPause)
}
```

- [ ] **Step 2: Rodar e ver falhar**

```bash
cd apps/player && swift test --filter DisplayTests 2>&1 | tail -15
```

Esperado: erro de compilação — `cannot find formatTime in scope`.

- [ ] **Step 3: Implementar `TimeFormat.swift`**

```swift
import Foundation

/// Relógio da faixa. Valores inválidos — negativos, NaN, infinito — viram `0:00` em vez de
/// texto quebrado na tela: `totalSeconds` chega zerado antes de qualquer carga, e uma taxa de
/// amostragem zerada produz NaN.
public func formatTime(_ seconds: Double) -> String {
    guard seconds.isFinite, seconds > 0 else { return "0:00" }
    let total = Int(seconds.rounded(.down))
    let horas = total / 3600
    let minutos = (total % 3600) / 60
    let segundos = total % 60
    if horas > 0 {
        return String(format: "%d:%02d:%02d", horas, minutos, segundos)
    }
    return String(format: "%d:%02d", minutos, segundos)
}
```

- [ ] **Step 4: Implementar `DisplayState.swift`**

```swift
import Foundation
import HogAudioBindings

public struct DisplayState: Equatable {
    public let title: String
    public let artist: String
    public let album: String
    public let artwork: Data?
    public let elapsed: String
    public let total: String
    public let progress: Double
    public let canPlay: Bool
    public let canPause: Bool
    public let isPlaying: Bool
    public let technicalLine: String
}

/// Traduz o estado do engine para o que a tela mostra. É função pura de propósito: a interface
/// consulta dez vezes por segundo, e nenhuma dessas consultas pode depender de hardware para
/// ser testada.
public func displayState(
    snapshot: Snapshot,
    metadata: TrackMetadata?,
    format: TrackFormat?
) -> DisplayState {
    let tocando = snapshot.state == .playing
    // Só estes dois estados seguram o device: em Loaded a negociação já rodou, mas o hog só
    // é tomado no play.
    let comDevice = snapshot.state == .playing || snapshot.state == .paused

    var tecnica = ""
    if let format {
        var partes = [
            String(format: "%.0f kHz", format.sampleRate / 1000),
            "\(format.bitDepth) bit",
            format.codec,
        ]
        if comDevice { partes.append("hog ativo") }
        if snapshot.underruns > 0 { partes.append("\(snapshot.underruns) underruns") }
        tecnica = partes.joined(separator: " · ")
    }

    let progresso: Double
    if snapshot.totalSeconds > 0 {
        progresso = min(1.0, max(0.0, snapshot.elapsedSeconds / snapshot.totalSeconds))
    } else {
        progresso = 0
    }

    return DisplayState(
        title: metadata?.title ?? "nenhuma faixa carregada",
        artist: metadata?.artist ?? "",
        album: metadata?.album ?? "",
        artwork: metadata?.artwork,
        elapsed: formatTime(snapshot.elapsedSeconds),
        total: formatTime(snapshot.totalSeconds),
        progress: progresso,
        canPlay: [.loaded, .paused, .finished].contains(snapshot.state),
        canPause: tocando,
        isPlaying: tocando,
        technicalLine: tecnica
    )
}
```

- [ ] **Step 5: Rodar e ver passar**

```bash
cd apps/player && swift test --filter DisplayTests 2>&1 | tail -12
```

Esperado: 9 testes passando. Se os inicializadores de `Snapshot` ou `TrackFormat` reclamarem de
rótulos, confira os nomes gerados em `Sources/HogAudioBindings/hog_audio.swift` — o gerado
manda.

- [ ] **Step 6: Provar que os testes conseguem reprovar**

Troque temporariamente em `displayState` a linha `let comDevice = snapshot.state == .playing ||
snapshot.state == .paused` por `let comDevice = true` e rode:

```bash
cd apps/player && swift test --filter DisplayTests 2>&1 | tail -12
```

Esperado: `carregadoAindaNaoSegurraODevice` **falha**. Desfaça e confirme o verde.

- [ ] **Step 7: Commit**

```bash
git add apps/player/Sources/HogPlayerKit/TimeFormat.swift \
        apps/player/Sources/HogPlayerKit/DisplayState.swift \
        apps/player/Tests/HogPlayerKitTests/DisplayTests.swift
git commit -m "feat(app): formatar tempo e traduzir estado do engine para a tela"
```

---

### Task 12: ViewModel com poll a 10 Hz

O `play()` pode levar centenas de milissegundos, porque adquire o hog, troca o rate e espera o
hardware confirmar. Chamá-lo na thread principal congelaria a janela justo no clique. Por isso
os comandos saem para uma tarefa de fundo e só o poll roda no main.

Para que a lógica de comando seja testável sem device, o ViewModel depende de um protocolo, não
da classe concreta gerada.

**Files:**
- Create: `apps/player/Sources/HogPlayerKit/PlayerViewModel.swift`, `apps/player/Tests/HogPlayerKitTests/ViewModelTests.swift`

**Interfaces:**
- Consumes: `DisplayState`, `displayState(snapshot:metadata:format:)` da Task 11; `loadMetadata(from:)` da Task 10; `HogPlayer` dos bindings.
- Produces: `protocol PlayerControlling`; `@MainActor final class PlayerViewModel: ObservableObject` com `display: DisplayState`, `volume: Double`, `errorMessage: String?`, e os métodos `open(url:)`, `toggle()`, `applyVolume(_:)`, `startPolling()`, `stopPolling()`, `shutdown()`.

- [ ] **Step 1: Escrever os testes que falham**

Crie `apps/player/Tests/HogPlayerKitTests/ViewModelTests.swift`:

```swift
import Testing
import Foundation
@testable import HogPlayerKit
import HogAudioBindings

/// Dublê que registra o que foi chamado. Permite testar a lógica de comando sem tomar o
/// device de áudio da máquina onde os testes rodam.
final class FakePlayer: PlayerControlling, @unchecked Sendable {
    var chamadas: [String] = []
    var estado: PlayerState = .idle
    var volumeAplicado: Float?

    func load(path: String) throws -> TrackFormat {
        chamadas.append("load")
        estado = .loaded
        return TrackFormat(sampleRate: 44100, bitDepth: 16, channels: 2,
                           codec: "flac", deviceName: "Fake", totalSeconds: 10)
    }
    func play() throws { chamadas.append("play"); estado = .playing }
    func pause() throws { chamadas.append("pause"); estado = .paused }
    func setVolume(scalar: Float) throws { volumeAplicado = scalar }
    func snapshot() -> Snapshot {
        Snapshot(state: estado, elapsedSeconds: 0, totalSeconds: 10,
                 underruns: 0, volumeScalar: 0.5)
    }
    func shutdown() { chamadas.append("shutdown") }
}

@Test @MainActor func oBotaoTocaQuandoParadoEPausaQuandoTocando() {
    let fake = FakePlayer()
    let vm = PlayerViewModel(player: fake)

    fake.estado = .loaded
    vm.refresh()
    vm.toggle()
    #expect(fake.chamadas.contains("play"))

    fake.estado = .playing
    vm.refresh()
    vm.toggle()
    #expect(fake.chamadas.contains("pause"))
}

@Test @MainActor func semFaixaCarregadaOBotaoNaoFazNada() {
    let fake = FakePlayer()
    let vm = PlayerViewModel(player: fake)
    vm.refresh()
    vm.toggle()
    #expect(fake.chamadas.isEmpty)
}

@Test @MainActor func oVolumeVaiDeZeroAUmParaOEngine() {
    let fake = FakePlayer()
    let vm = PlayerViewModel(player: fake)
    vm.applyVolume(0.35)
    #expect(fake.volumeAplicado == 0.35)
}

@Test @MainActor func oRefreshAtualizaOQueATelaMostra() {
    let fake = FakePlayer()
    fake.estado = .playing
    let vm = PlayerViewModel(player: fake)
    vm.refresh()
    #expect(vm.display.isPlaying)
    #expect(vm.display.canPause)
}
```

- [ ] **Step 2: Rodar e ver falhar**

```bash
cd apps/player && swift test --filter ViewModelTests 2>&1 | tail -15
```

Esperado: erro de compilação — `cannot find PlayerControlling in scope`.

- [ ] **Step 3: Implementar**

Crie `apps/player/Sources/HogPlayerKit/PlayerViewModel.swift`:

```swift
import Foundation
import HogAudioBindings

/// O ViewModel fala com este protocolo, não com a classe gerada, para que a lógica de comando
/// possa ser testada sem tomar o device de áudio da máquina.
public protocol PlayerControlling: AnyObject {
    func load(path: String) throws -> TrackFormat
    func play() throws
    func pause() throws
    func setVolume(scalar: Float) throws
    func snapshot() -> Snapshot
    func shutdown()
}

extension HogPlayer: PlayerControlling {}

@MainActor
public final class PlayerViewModel: ObservableObject {
    @Published public private(set) var display: DisplayState
    @Published public var volume: Double = 0.5
    @Published public private(set) var errorMessage: String?

    private let player: PlayerControlling
    private var metadata: TrackMetadata?
    private var format: TrackFormat?
    private var timer: Timer?

    public init(player: PlayerControlling) {
        self.player = player
        self.display = displayState(
            snapshot: player.snapshot(), metadata: nil, format: nil
        )
    }

    public convenience init() {
        self.init(player: HogPlayer())
    }

    public func startPolling() {
        // Dez vezes por segundo: suficiente para o relógio parecer contínuo, e barato porque
        // o snapshot só lê atômicos do lado Rust.
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.refresh() }
        }
    }

    public func stopPolling() {
        timer?.invalidate()
        timer = nil
    }

    public func refresh() {
        display = displayState(
            snapshot: player.snapshot(), metadata: metadata, format: format
        )
    }

    public func open(url: URL) {
        errorMessage = nil
        let tags = Task { await loadMetadata(from: url) }
        do {
            format = try player.load(path: url.path)
            metadata = nil
            Task { @MainActor in
                metadata = await tags.value
                refresh()
            }
            volume = Double(player.snapshot().volumeScalar)
            refresh()
        } catch {
            format = nil
            metadata = nil
            errorMessage = "\(error)"
            refresh()
        }
    }

    public func toggle() {
        let deveTocar = display.canPlay
        let devePausar = display.canPause
        guard deveTocar || devePausar else { return }

        // Fora da thread principal: adquirir o hog e esperar o rate efetivar leva centenas de
        // milissegundos, e a janela não pode congelar no clique.
        Task.detached { [player] in
            do {
                if devePausar {
                    try player.pause()
                } else {
                    try player.play()
                }
            } catch {
                await MainActor.run { self.errorMessage = "\(error)" }
            }
            await MainActor.run { self.refresh() }
        }
    }

    public func applyVolume(_ scalar: Double) {
        volume = scalar
        try? player.setVolume(scalar: Float(scalar))
    }

    public func shutdown() {
        stopPolling()
        player.shutdown()
    }
}
```

- [ ] **Step 4: Rodar e ver passar**

```bash
cd apps/player && swift test --filter ViewModelTests 2>&1 | tail -12
```

Esperado: 4 testes passando.

- [ ] **Step 5: Rodar a suíte Swift inteira**

```bash
cd apps/player && swift test 2>&1 | tail -10
```

Esperado: todos os testes das Tasks 9 a 12 passando.

- [ ] **Step 6: Commit**

```bash
git add apps/player/Sources/HogPlayerKit/PlayerViewModel.swift \
        apps/player/Tests/HogPlayerKitTests/ViewModelTests.swift
git commit -m "feat(app): adicionar viewmodel com poll e comandos assincronos"
```

---

### Task 13: A janela

Interface utilitária de propósito. O objetivo declarado desta versão é núcleo funcional, não
interface bonita.

**Files:**
- Create: `apps/player/Sources/HogPlayer/HogPlayerApp.swift`, `apps/player/Sources/HogPlayer/ContentView.swift`

**Interfaces:**
- Consumes: `PlayerViewModel` da Task 12.
- Produces: o executável `HogPlayer`.

- [ ] **Step 1: Escrever `ContentView.swift`**

```swift
import AppKit
import HogPlayerKit
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @ObservedObject var model: PlayerViewModel

    var body: some View {
        VStack(spacing: 14) {
            capa
            informacoes
            relogio
            controles
            volume
            rodape
        }
        .padding(18)
        .frame(width: 360, height: 480)
        .onDrop(of: [.fileURL], isTargeted: nil, perform: receberArquivo)
    }

    private var capa: some View {
        Group {
            if let dados = model.display.artwork, let imagem = NSImage(data: dados) {
                Image(nsImage: imagem).resizable().aspectRatio(contentMode: .fit)
            } else {
                RoundedRectangle(cornerRadius: 6)
                    .fill(Color.secondary.opacity(0.15))
                    .overlay(Image(systemName: "music.note").font(.largeTitle)
                        .foregroundStyle(.secondary))
            }
        }
        .frame(width: 220, height: 220)
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    private var informacoes: some View {
        VStack(spacing: 3) {
            Text(model.display.title).font(.headline).lineLimit(1)
            Text(model.display.artist).font(.subheadline)
                .foregroundStyle(.secondary).lineLimit(1)
            Text(model.display.album).font(.caption)
                .foregroundStyle(.tertiary).lineLimit(1)
        }
    }

    private var relogio: some View {
        VStack(spacing: 4) {
            ProgressView(value: model.display.progress)
            HStack {
                Text(model.display.elapsed)
                Spacer()
                Text(model.display.total)
            }
            .font(.caption.monospacedDigit())
            .foregroundStyle(.secondary)
        }
    }

    private var controles: some View {
        HStack(spacing: 20) {
            Button("Abrir…", action: escolherArquivo)
            Button(action: model.toggle) {
                Image(systemName: model.display.isPlaying ? "pause.fill" : "play.fill")
                    .font(.title)
                    .frame(width: 44, height: 44)
            }
            .disabled(!model.display.canPlay && !model.display.canPause)
            .keyboardShortcut(.space, modifiers: [])
        }
    }

    private var volume: some View {
        HStack {
            Image(systemName: "speaker.fill").foregroundStyle(.secondary)
            Slider(value: Binding(
                get: { model.volume },
                set: { model.applyVolume($0) }
            ), in: 0...1)
            Text("\(Int(model.volume * 100))%")
                .font(.caption.monospacedDigit())
                .frame(width: 40, alignment: .trailing)
        }
    }

    private var rodape: some View {
        VStack(spacing: 4) {
            Text(model.display.technicalLine)
                .font(.caption2.monospaced())
                .foregroundStyle(.secondary)
            if let erro = model.errorMessage {
                Text(erro).font(.caption2).foregroundStyle(.red)
                    .lineLimit(3).multilineTextAlignment(.center)
            }
        }
        .frame(height: 48)
    }

    private func escolherArquivo() {
        let painel = NSOpenPanel()
        painel.allowsMultipleSelection = false
        painel.canChooseDirectories = false
        painel.allowedContentTypes = [.audio]
        if painel.runModal() == .OK, let url = painel.url {
            model.open(url: url)
        }
    }

    private func receberArquivo(_ provedores: [NSItemProvider]) -> Bool {
        guard let provedor = provedores.first else { return false }
        _ = provedor.loadObject(ofClass: URL.self) { url, _ in
            guard let url else { return }
            Task { @MainActor in model.open(url: url) }
        }
        return true
    }
}
```

- [ ] **Step 2: Escrever `HogPlayerApp.swift`**

O `shutdown` no encerramento é obrigatório: sem ele o device fica com o rate, o formato e o
volume da última faixa. O sistema solta o hog sozinho quando o processo morre, mas não desfaz a
configuração.

```swift
import AppKit
import HogPlayerKit
import SwiftUI

@main
struct HogPlayerApp: App {
    @StateObject private var model = PlayerViewModel()
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        Window("hog-audio", id: "player") {
            ContentView(model: model)
                .onAppear {
                    delegate.model = model
                    model.startPolling()
                    // Argumento de linha de comando: é o que permite iterar rápido durante o
                    // desenvolvimento sem passar pelo painel de abrir arquivo.
                    if let caminho = CommandLine.arguments.dropFirst().first {
                        model.open(url: URL(fileURLWithPath: caminho))
                    }
                }
        }
        .windowResizability(.contentSize)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    @MainActor var model: PlayerViewModel?

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func applicationWillTerminate(_ notification: Notification) {
        // Sem isto o device fica travado no rate e no formato da última faixa.
        MainActor.assumeIsolated { model?.shutdown() }
    }
}
```

- [ ] **Step 3: Compilar**

```bash
cd apps/player && swift build -c release 2>&1 | tail -20
```

Esperado: compila. Erros de nome de símbolo do Core Audio indicam que os frameworks não foram
declarados no `Package.swift` — reveja a Task 9, Step 2.

- [ ] **Step 4: Rodar e verificar à mão**

```bash
cd apps/player && ./.build/release/HogPlayer ../../musicas/Skyfall.flac
```

Verifique, nesta ordem:

1. A janela abre com a **capa** do álbum, título "Skyfall" e artista "Adele".
2. A linha técnica mostra a taxa, os bits e o codec, **sem** "hog ativo" — nada foi sequestrado
   ainda.
3. Ao clicar em play: sai som, a linha passa a mostrar "hog ativo" e o relógio avança.
4. Ao clicar em pause: o som para e **o relógio congela**.
5. Ao clicar em play de novo: o som volta de onde parou, sem pulo nem estalo.
6. O slider de volume muda o volume de verdade enquanto toca.
7. Ao fechar a janela: o áudio do sistema volta ao normal.

- [ ] **Step 5: Commit**

```bash
git add apps/player/Sources/HogPlayer
git commit -m "feat(app): adicionar janela com play, pause, volume e metadados"
```

---

### Task 14: Bundle `.app` e verificação final

**Files:**
- Create: `apps/player/Resources/Info.plist`
- Modify: `Makefile`, `README.md`

**Interfaces:**
- Consumes: o executável da Task 13.
- Produces: `make app` gerando `apps/player/HogAudio.app`, e `make run-app`.

- [ ] **Step 1: Escrever o `Info.plist`**

Crie `apps/player/Resources/Info.plist`. Não há entitlement de sandbox de propósito: hog mode
não sobrevive ao sandbox.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>HogAudio</string>
    <key>CFBundleDisplayName</key><string>hog-audio</string>
    <key>CFBundleIdentifier</key><string>local.hogaudio.player</string>
    <key>CFBundleExecutable</key><string>HogAudio</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
```

- [ ] **Step 2: Acrescentar os alvos ao `Makefile`**

```makefile
APP_DIR := apps/player/HogAudio.app

app: bindings
	@cd apps/player && swift build -c release
	@mkdir -p $(APP_DIR)/Contents/MacOS
	@cp apps/player/Resources/Info.plist $(APP_DIR)/Contents/Info.plist
	@cp apps/player/.build/release/HogPlayer $(APP_DIR)/Contents/MacOS/HogAudio
	@echo "app montado em $(APP_DIR)"

# make run-app FILE=musicas/faixa.flac
run-app: app
	@$(APP_DIR)/Contents/MacOS/HogAudio "$(FILE)"
```

Acrescente `app run-app` à linha `.PHONY`.

- [ ] **Step 3: Montar e abrir pelo Finder**

```bash
make app
open apps/player/HogAudio.app
```

Esperado: o app abre com ícone próprio no Dock e aceita foco de teclado — é a diferença entre
um binário solto e um bundle.

- [ ] **Step 4: Rodar todos os gates de uma vez**

```bash
make test
make verify FILE=testdata/t96_24.flac BITS=24
make verify-pause FILE=testdata/t96_24.flac
make verify-volume FILE=testdata/t96_24.flac
cd rust && cargo test -- --ignored --test-threads=1 2>&1 | tail -10
```

Esperado: tudo verde. Este é o conjunto completo: testes puros de Rust e Swift, testes de
hardware, e as três provas de bit-perfect.

- [ ] **Step 5: Verificar com fone, arquivo real e as quatro faixas**

Conecte o fone antes de começar — o player usa o device de saída padrão no momento do `load`.

```bash
make run-app FILE=musicas/Skyfall.flac
```

Confirme, faixa a faixa:

1. `Skyfall.flac` e `Heat-Waves.flac` **mostram capa**; `05-Death-of-a-Bachelor.flac` e
   `10-House-of-Memories.flac` **não têm capa embutida** e devem cair no placeholder, com
   título e artista ainda corretos.
2. Durante a reprodução, tocar algo no navegador ou no Music: **tem de ficar mudo**. É a prova
   do hog.
3. Pausar e esperar meio minuto: o relógio não anda, e o resto do Mac continua mudo — é o
   comportamento decidido, não defeito.
4. Retomar: o som volta exatamente de onde parou.
5. Abrir outra faixa **com o player tocando**: a anterior para, o device é restaurado e a nova
   carrega. Se as taxas forem diferentes (96 kHz contra 44,1 kHz), o novo play trava o device
   na taxa nova.
6. Fechar o app e confirmar, em outro terminal, que o device voltou ao normal:
   ```bash
   system_profiler SPAudioDataType | grep -A4 "Fones de Ouvido"
   ```

- [ ] **Step 6: Atualizar o `README.md`**

Acrescente uma seção descrevendo: como montar (`make app`), o que o player faz e o que não faz
(sem seek, sem lista, uma faixa por vez), que o Mac fica mudo enquanto o player segura o device
— inclusive pausado —, que force-quit deixa o sample rate trocado, e que tirar o fone durante a
reprodução não é tratado nesta versão: o tempo congela e a saída é fechar o player.

- [ ] **Step 7: Commit**

```bash
git add apps/player/Resources/Info.plist Makefile README.md
git commit -m "feat(app): montar bundle do app e documentar o uso"
```

---

## Verificação final do trabalho inteiro

Antes de considerar concluído, todos estes têm de valer ao mesmo tempo:

| Prova | Comando | Resultado esperado |
|---|---|---|
| Testes puros de Rust | `cd rust && cargo test` | Todos passam; total ≥ o anotado na Task 1 |
| Testes de hardware | `cd rust && cargo test -- --ignored --test-threads=1` | Todos passam |
| Testes de Swift | `cd apps/player && swift test` | Todos passam |
| Bit-perfect | `make verify FILE=testdata/t96_24.flac BITS=24` | Três comparações batem |
| Continuidade da pausa | `make verify-pause FILE=testdata/t96_24.flac` | Fluxo idêntico |
| Volume não toca nas amostras | `make verify-volume FILE=testdata/t96_24.flac` | Amostras idênticas |
| CLI intacta | `./rust/target/release/hog-audio --volume 20 testdata/t44_16.flac` | Toca e restaura |
| App | `make app && open apps/player/HogAudio.app` | Abre, toca, pausa, restaura |

**Se `make verify` divergir em qualquer ponto do plano, pare.** Essa é a única prova que
sustenta a premissa do projeto inteiro, e ela vale mais do que qualquer funcionalidade nova
deste plano.
