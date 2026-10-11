# Protótipo GPUI: plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Construir em `apps/gpui/` um player em GPUI com paridade mínima com o app SwiftUI, sobre o mesmo core `rust/` sem mudar uma linha dele, e medir os dois com os critérios combinados.

**Architecture:** Um workspace Cargo próprio em `apps/gpui/`, com dois crates:
- `hog-player-model` é o controlador, sem GPUI (isso é garantido pelo `Cargo.toml` dele). Ele porta as regras do `PlayerViewModel.swift` sobre uma trait `Player` que o `HogPlayer` do core implementa. Também guarda os estados puros e a leitura de tags e capas.
- `hog-player-gpui` é a UI em GPUI (Zed v1.21.0 por git). Ela só desenha o modelo e manda intenções.

O core entra por `path` e só pela API pública.

**Tech Stack:**
- Rust edition 2024;
- `gpui` e `gpui_platform` do Zed na tag `v1.21.0`;
- `lofty =0.25.4` para tags;
- `image 0.25` para miniatura e fundo desfocado;
- `signal-hook 0.4` para os sinais;
- Swift (scripts) para os medidores externos.

**Spec:** `docs/superpowers/specs/2026-09-27-prototipo-gpui-design.md`

## Global Constraints

- `git diff main -- rust/` fica vazio e `git status --porcelain -- rust/` fica sem saída do começo ao fim. O core não muda para servir o protótipo.
- Nada toca IOProc, ring, decodificador, negociação nem a ordem hog → rate → formato → volume. `make verify*` roda antes de qualquer instalação. Este plano não instala nada.
- O protótipo só usa a API pública de `hog_audio::api` (`HogPlayer`, `probe_track`, os tipos `Snapshot`, `QueueItem`, `TrackFormat`, `TrackProbe`, `TransitionTimings`, `PlayerError`) e `hog_audio::transitions::PlayerState`.
- `hog-player-model` não depende de `gpui` nem de nada de UI.
- O workspace de `apps/gpui/` tem `Cargo.lock` e `target/` próprios. Nunca vira workspace com o `rust/`.
- `coreaudio-sys` e `uniffi` saem, no `apps/gpui/Cargo.lock`, nas mesmas versões do `rust/Cargo.lock`. O mesmo vale para todo pacote da árvore do core (nome, versão e features), salvo as exceções listadas com motivo em `apps/gpui/tools/core_version_exceptions.txt`; `coreaudio-sys` e `uniffi` nunca são exceção.
- O GPUI vem de `git = "https://github.com/zed-industries/zed", tag = "v1.21.0"`, sem `gpui-component` e sem `gpui-pre`.
- Tecnologia web está fora.
- Os testes ficam em `tests/` de cada crate, fora de `src/`, por regra do usuário. Os nomes dos testes vão em pt-BR, com `snake_case`, como no core.
- O código vai em inglês. Strings de UI e mensagens, em pt-BR.
- TDD: todo teste é visto falhando antes do código, e cada tarefa tem uma sabotagem que prova que o teste pega o defeito.
- Gate de cada tarefa: `make gpui-test` verde e `make test` verde (core e Swift, intocados).
- Commits só quando o usuário pedir. Os passos "Commit" deste plano ficam como pontos de parada: o executor mostra o `git status` e espera. Quando o usuário autorizar, o commit segue o commit-guard:
  - Conventional Commits em pt-BR, no infinitivo e com escopo;
  - assunto sem acento, com até 72 caracteres;
  - no máximo 20 arquivos por commit;
  - `-m` obrigatório;
  - backup do working tree antes, porque um bloqueio descarta o working tree.
- Tudo que toma o device de áudio (tocar pelo app, `make hw-test`, `make verify*` e as medições 1, 2, 5, 6, 8 e 9) só roda depois de avisar o usuário e ter o ok dele.
- Leitura de arquivos com Read; criação e alteração com Write e Edit. O Bash serve para executar comandos.

## Review Focus

1. **O mesmo arquivo duas vezes na fila.** As linhas carregadas são por caminho, como no Swift. As duas linhas mostram os mesmos dados, e remover uma não pode apagar os dados da outra. Teste: `a_mesma_faixa_duas_vezes_divide_a_linha_e_remover_uma_nao_apaga_a_outra`, na Task 4.
2. **Arquivo que some ou não abre entre entrar na fila e carregar.** A linha mostra o nome do arquivo e o motivo no tooltip, sem pânico. Testes: `arquivo_que_nao_abre_vira_problema_com_a_mensagem_do_core` e `arquivo_que_nao_abre_vira_o_nome_do_arquivo`, nas Tasks 3 e 2.
3. **Capa enorme ou corrompida.** Uma capa de 3000 px vira miniatura de 96 px e capa de no máximo 1200 px. Bytes que não são imagem viram "sem capa". Nada disso pode travar a carga nem entrar em pânico. Testes: `capa_enorme_vira_no_maximo_1200` e `bytes_que_nao_sao_imagem_nao_viram_capa`, na Task 2, e `raster_invalido_nao_vira_imagem`, na Task 7.
4. **Tag vazia ou só com espaços.** Vale o nome do arquivo como título, e artista ou álbum vazio não aparece. Teste: `texto_vazio_ou_so_espacos_nao_vale`, na Task 2.
5. **Fechar o app com um comando segurando o device.** O shutdown descarta o que ainda não começou e chama o `shutdown` do core, que espera o comando em curso pelo lock de comandos antes de devolver o device. O pânico tenta o mesmo, com teto. Testes: `shutdown_descarta_o_que_ainda_nao_chegou_ao_player`, na Task 4, e `o_panico_tenta_devolver_o_device_com_teto`, na Task 9. A espera pelo lock é do core e já tem teste lá (`shutdown_fecha_a_fronteira_para_comandos_posteriores`).

---

## Estrutura de arquivos

```
apps/gpui/
  Cargo.toml                      workspace (members: model, app, testing)
  Cargo.lock                      semeado com o do Zed; coreaudio-sys e uniffi iguais aos do core
  .gitignore                      target/, build/, bench/build/
  tools/check_core_untouched.sh   gate: core intocado e versões iguais às do core
  tools/make_app.sh               monta o .app em apps/gpui/build/
  testing/                        hog-player-testing (só teste)
    src/lib.rs                    FakePlayer, Gate, config_de_teste e helpers
  model/                          hog-player-model (sem GPUI)
    Cargo.toml
    src/lib.rs
    src/port.rs                   trait Player, impl para HogPlayer, Track, Format, Timings
    src/format.rs                 format_time, quality_detail, compact_quality
    src/display.rs                DisplayState, ClockState, display_state, clock_state
    src/layout.rs                 cover_side e constantes (porte de PlayerLayout.swift)
    src/audio_files.rs            expansão de pastas e filtro de áudio
    src/media/mod.rs              Raster, Artwork, ArtworkRef, Thumbnail
    src/media/artwork.rs          thumbnail, cover, backdrop (BGRA)
    src/media/tags.rs             Tags, read_tags (lofty)
    src/entry.rs                  QueueEntryInfo, ProbeOutcome, queue_entry_info, load_queue_entry
    src/metadata.rs               TrackMetadata, load_track_metadata
    src/controller/mod.rs         Controller (API pública e tick)
    src/controller/model.rs       Model, QueueState, VolumeState, Changes
    src/controller/commands.rs    Job, Outcome, thread de comandos
    src/controller/volume.rs      worker de volume
    src/controller/loaders.rs     carga das linhas (4 threads) e dos metadados da atual
    tests/support/mod.rs          fixture() e snapshot() dos testes puros
    tests/format.rs  tests/display.rs  tests/layout.rs  tests/audio_files.rs
    tests/media.rs   tests/entry.rs
    tests/controller_transport.rs tests/controller_volume.rs tests/controller_queue.rs
    tests/controller_observation.rs
  app/                            hog-player-gpui (UI)
    Cargo.toml                    feature panic-probe (medição 9)
    Resources/Info.plist
    src/lib.rs                    módulos da UI, testáveis
    src/main.rs                   bootstrap: aplicação, janela, sinais, pânico, on_app_quit
    src/store.rs                  Store: Controller, sinais por parte do modelo, poll e wake
    src/actions.rs                actions!, atalhos, menus e o painel de abrir
    src/exit.rs                   log em arquivo, sinais e panic hook
    src/images.rs                 Raster -> RenderImage, cache de miniaturas, ArtworkSlot
    src/theme.rs                  tokens de cor, espaço e tipo
    src/views/mod.rs              observe(): cada view assina só o seu sinal
    src/views/root.rs             duas colunas, barra com Abrir, arrastar do Finder
    src/views/cover.rs            capa
    src/views/backdrop.rs         fundo desfocado com cruzamento
    src/views/track_info.rs       título, artista, álbum
    src/views/clock.rs            relógio
    src/views/transport.rs        anterior, tocar/pausar, próxima
    src/views/volume.rs           slider (adaptado do Pawse) e mudo
    src/views/technical.rs        linha técnica
    src/views/status.rs           aviso de erro e de perda do device
    src/views/queue.rs            uniform_list, linhas, seleção, Delete, Limpar
    src/views/tooltip.rs          a view da dica
    src/views/icons.rs            ícones SVG próprios
    tests/views.rs                #[gpui::test] do Store e das ações
    tests/images.rs               conversão e cache de imagens
    tests/exit.rs                 log, sinais e pânico (binário próprio)
  bench/
    window_timer.swift            medição 4
    input_probe.swift             medições 5, 8 e 9
    README.md                     como rodar cada medição
Makefile                          alvos gpui, gpui-fmt, gpui-test, gpui-app
```

As tarefas de UI (5 a 10) usam a API do GPUI da tag `v1.21.0`, levantada em
`docs/research/2026-09-27-gpui-v1.21-api.md`. A Task 0 compila o GPUI fora do Zed e prova o
`test-support` antes de a UI começar.

### Task 0: Spike: workspace, GPUI fora do Zed, gate do core

Sem device. Se algum passo reprovar, o trabalho para e o problema vai ao usuário.

**Files:**
- Create: `apps/gpui/Cargo.toml`, `apps/gpui/Cargo.lock` (semeado), `apps/gpui/.gitignore`
- Create: `apps/gpui/model/Cargo.toml`, `apps/gpui/model/src/lib.rs`
- Create: `apps/gpui/app/Cargo.toml`, `apps/gpui/app/src/lib.rs`, `apps/gpui/app/src/main.rs`
- Create: `apps/gpui/app/tests/views.rs`
- Create: `apps/gpui/tools/check_core_untouched.sh`
- Modify: `Makefile` (só acrescentar alvos no fim; nenhuma linha existente muda)
- Modify: `docs/research/2026-09-27-gpui-v1.21-api.md` (acrescentar a seção "Conferido no passo 0")

**Interfaces:**
- Produces:
  - o workspace `apps/gpui`, com os membros `model` e `app`;
  - `make gpui`, `make gpui-fmt`, `make gpui-test`;
  - `apps/gpui/tools/check_core_untouched.sh`, que termina em 0 só com o core intocado e as versões iguais às do core.

- [ ] **Step 1: Branch e estado de partida**

Run:
```bash
git status --short && git log --oneline -1
git switch -c proto/gpui
```
Expected: `a11fe30` no topo. Só aparecem como untracked a spec, este plano e `docs/research/2026-09-27-gpui-v1.21-api.md`; nada em `rust/` nem em `apps/player/`.

- [ ] **Step 2: Metal Toolchain**

Run: `xcrun -sdk macosx metal --version`
Expected: a versão do compilador Metal. Se aparecer "missing Metal Toolchain", **parar e pedir ao usuário** para rodar `! xcodebuild -downloadComponent MetalToolchain`. Não usar `runtime_shaders`: ela compila os shaders a cada abertura e distorce a medição 4.

- [ ] **Step 3: O gate do core, primeiro vermelho**

`apps/gpui/tools/check_core_untouched.sh`:

```bash
#!/usr/bin/env bash
# Gate do protótipo: o core não muda para servir a UI, e o core compilado dentro do protótipo
# usa as mesmas versões de coreaudio-sys e uniffi que o `make verify*` provou.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"

if ! git -C "$root" diff --quiet main -- rust/; then
  echo "FALHOU: o core mudou em relação à main (git diff main -- rust/)"
  exit 1
fi
if [ -n "$(git -C "$root" status --porcelain -- rust/)" ]; then
  echo "FALHOU: há arquivos novos ou alterados em rust/"
  exit 1
fi

lock_version() {
  awk -v name="$2" '$0 == "name = \"" name "\"" { getline; gsub(/"/, "", $3); print $3; exit }' "$1"
}

for crate in coreaudio-sys uniffi; do
  core="$(lock_version "$root/rust/Cargo.lock" "$crate")"
  proto="$(lock_version "$root/apps/gpui/Cargo.lock" "$crate")"
  if [ -z "$core" ] || [ "$core" != "$proto" ]; then
    echo "FALHOU: $crate é '$core' no core e '$proto' no protótipo"
    exit 1
  fi
done

echo "core intocado; coreaudio-sys e uniffi iguais aos do core"
```

Run: `chmod +x apps/gpui/tools/check_core_untouched.sh && apps/gpui/tools/check_core_untouched.sh`
Expected: `FALHOU: coreaudio-sys é '0.2.x' no core e '' no protótipo`. Ainda não há lockfile do protótipo; esse é o vermelho do gate.

- [ ] **Step 4: Workspace e crates mínimos**

`apps/gpui/Cargo.toml`:

```toml
# Protótipo de UI em GPUI. Workspace próprio, com lock e target próprios: nunca vira workspace
# com o `rust/`, para o lockfile do core não mudar. Spec:
# docs/superpowers/specs/2026-09-27-prototipo-gpui-design.md
[workspace]
resolver = "3"
members = ["model", "app"]

[workspace.package]
version = "0.1.0"
edition = "2024"
publish = false

[workspace.dependencies]
hog-audio = { path = "../../rust" }
gpui = { git = "https://github.com/zed-industries/zed", tag = "v1.21.0" }
# Sem `font-kit` o gpui_macos cai no NoopTextSystem e a janela abre sem texto.
gpui_platform = { git = "https://github.com/zed-industries/zed", tag = "v1.21.0", features = ["font-kit"] }
image = { version = "0.25.10", default-features = false, features = ["jpeg", "png"] }
lofty = "=0.25.4"
signal-hook = "0.4.4"

# Réplica do [patch.crates-io] do workspace do Zed (Cargo.toml:996-1021 na v1.21.0), só com o que
# entra na árvore do macOS. Fora do workspace do Zed o patch dele não vale. Lista conferida na
# Task 0, Step 6.
[patch.crates-io]
```

`apps/gpui/.gitignore`:

```
target/
build/
```

`apps/gpui/model/Cargo.toml`:

```toml
[package]
name = "hog-player-model"
version.workspace = true
edition.workspace = true
publish.workspace = true

[dependencies]
hog-audio.workspace = true
image.workspace = true
lofty.workspace = true
```

`apps/gpui/model/src/lib.rs`:

```rust
//! O player sem interface: a fronteira com o core, os estados que a tela mostra e o
//! controlador que porta as regras do `PlayerViewModel.swift`. Nada aqui depende de GPUI.
```

`apps/gpui/app/Cargo.toml`:

```toml
[package]
name = "hog-player-gpui"
version.workspace = true
edition.workspace = true
publish.workspace = true

[lib]
path = "src/lib.rs"

[[bin]]
name = "hog-player-gpui"
path = "src/main.rs"

[dependencies]
gpui.workspace = true
gpui_platform.workspace = true

[dev-dependencies]
gpui = { workspace = true, features = ["test-support"] }
```

`apps/gpui/app/src/lib.rs`:

```rust
//! A interface em GPUI. Desenha o modelo do `hog-player-model` e manda intenções; nenhuma
//! regra de reprodução mora aqui.
```

`apps/gpui/app/src/main.rs` (o spike: uma janela com texto, sem player):

```rust
use gpui::{
    App, AppContext, Bounds, Context, IntoElement, ParentElement, Render, Styled, Window,
    WindowBounds, WindowOptions, div, px, rgb, size,
};

struct Spike;

impl Render for Spike {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xffffff))
            .child("hog-audio GPUI: spike")
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(920.0), px(700.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Spike),
        )
        .expect("não consegui abrir a janela");
        cx.activate(true);
    });
}
```

`apps/gpui/app/tests/views.rs`:

```rust
use gpui::{AppContext, TestAppContext};

/// Prova que o `#[gpui::test]` e o `TestAppContext` funcionam com o GPUI vindo por git, fora do
/// workspace do Zed. Se reprovar por compilação, os testes de UI viram verificação manual.
#[gpui::test]
fn o_test_support_do_gpui_funciona_fora_do_zed(cx: &mut TestAppContext) {
    let contador = cx.new(|_| 0_u32);
    contador.update(cx, |n, _| *n += 1);
    assert_eq!(contador.read_with(cx, |n, _| *n), 1);
}
```

- [ ] **Step 5: Semear o lockfile com o do Zed e resolver**

Run:
```bash
curl -fsSL -o apps/gpui/Cargo.lock https://raw.githubusercontent.com/zed-industries/zed/v1.21.0/Cargo.lock
cd apps/gpui && cargo tree -p hog-player-gpui --target aarch64-apple-darwin -e normal,build > /dev/null
```
Expected: o cargo baixa o repositório do Zed, cerca de 527 MB na primeira vez, e resolve sem erro. Ele reaproveita as versões do lock do Zed (a pesquisa mediu 3 diferenças, todas fora do macOS).

- [ ] **Step 6: Replicar os patches do Zed que entram na árvore do macOS**

Run:
```bash
cd apps/gpui
curl -fsSL https://raw.githubusercontent.com/zed-industries/zed/v1.21.0/Cargo.toml | sed -n '996,1021p'
for c in tree-sitter-language async-process async-task windows-capture calloop livekit libwebrtc notify notify-types webrtc-sys scratch; do
  cargo tree -p hog-player-gpui --target aarch64-apple-darwin -e normal,build -i "$c" 2>/dev/null | head -1
done
```
Expected: pelo menos `async-task v4.7.1` na árvore (a pesquisa confirmou). Para cada crate listado, copiar a linha correspondente do `[patch.crates-io]` do Zed para o `[patch.crates-io]` de `apps/gpui/Cargo.toml`, com Edit. Para o `async-task`:

```toml
async-task = { git = "https://github.com/smol-rs/async-task", rev = "b4486cd71e4e94fbda54ce6302444de14f4d190e" }
```

`scratch` é patch por `path` dentro do Zed e não pode ser replicado. Se ele aparecer na árvore, registrar como diferença conhecida. Depois, rodar de novo o `cargo tree` do Step 5 e ver o `async-task` sair do git (`cargo tree -i async-task` mostra `(https://github.com/smol-rs/async-task?rev=b4486cd...)`).

- [ ] **Step 7: Alinhar coreaudio-sys e uniffi com o core e ver o gate passar**

Run:
```bash
awk '$0 == "name = \"coreaudio-sys\"" || $0 == "name = \"uniffi\"" { print; getline; print }' rust/Cargo.lock
awk '$0 == "name = \"coreaudio-sys\"" || $0 == "name = \"uniffi\"" { print; getline; print }' apps/gpui/Cargo.lock
```
O `hog-player-model` depende do `hog-audio` desde o Step 4, e o cargo resolve o lockfile do workspace inteiro, então o Step 5 já trouxe `coreaudio-sys` e `uniffi`. Se as versões diferirem das do core:

```bash
cd apps/gpui && cargo update -p coreaudio-sys --precise <versão do core> && cargo update -p uniffi --precise <versão do core>
```

Run: `apps/gpui/tools/check_core_untouched.sh`
Expected: `core intocado; coreaudio-sys e uniffi iguais aos do core`.

- [ ] **Step 8: Alvos do Makefile**

Acrescentar no fim do `Makefile` (com Edit, sem tocar nas linhas existentes):

```make

# --- Protótipo GPUI (apps/gpui): workspace próprio, com lock e target próprios. ---
# O gate reprova se o core mudar ou se o protótipo compilar outra versão de coreaudio-sys/uniffi.
GPUI_DIR := apps/gpui

.PHONY: gpui gpui-fmt gpui-test gpui-app

gpui:
	@cd $(GPUI_DIR) && cargo build --release -p hog-player-gpui

gpui-fmt:
	@cd $(GPUI_DIR) && cargo fmt --all

gpui-test:
	@$(GPUI_DIR)/tools/check_core_untouched.sh
	@cd $(GPUI_DIR) && cargo fmt --all --check
	@cd $(GPUI_DIR) && cargo clippy --workspace --all-targets -- -D warnings
	@cd $(GPUI_DIR) && cargo test --workspace
```

O alvo `gpui-app` é acrescentado na Task 11.

- [ ] **Step 9: Build, janela e teste do test-support**

Run:
```bash
cd apps/gpui && time cargo build --release -p hog-player-gpui
cd apps/gpui && cargo test -p hog-player-gpui --test views
```
Expected: o build termina, com o tempo anotado para a medição 10, e o teste `o_test_support_do_gpui_funciona_fora_do_zed` passa.

- Se o build falhar em `async-task`, conferir o patch do Step 6.
- Se falhar em `bindgen`/`libclang` (crate `media`), rodar `xcrun --find clang` e registrar o erro.
- Se o teste falhar por compilação, registrar a mensagem: os testes de UI das tarefas seguintes viram verificação manual, e o `[dev-dependencies]` com `test-support` sai do `app/Cargo.toml`.

Run: `cd apps/gpui && cargo run --release -p hog-player-gpui`
Expected: uma janela de 920×700 com o texto "hog-audio GPUI: spike" centralizado. Conferir com um screenshot (macos-use) ou pedir ao usuário. **Texto ausente** significa que a feature `font-kit` não chegou ao `gpui_macos`. Fechar com ⌘Q.

- [ ] **Step 10: Conferências que decidem as próximas tarefas**

Run:
```bash
cd apps/gpui
awk '$0 == "name = \"objc2\"" { getline; print }' Cargo.lock
gh issue view 861 --repo madsmtm/objc2 --json title,state,body --jq '.title, .state, .body' | head -40
cargo tree -p hog-player-gpui --target aarch64-apple-darwin -e normal --prefix none | sort -u | wc -l
ls -l target/release/hog-player-gpui
```

Acrescentar ao fim de `docs/research/2026-09-27-gpui-v1.21-api.md` uma seção `## Conferido no passo 0 (<data>)`, com fatos e sem tabelas:
- se o Metal Toolchain estava presente ou foi baixado;
- quais patches do Zed foram replicados e quais ficaram de fora;
- as versões de `objc2`, `async-task`, `coreaudio-sys` e `uniffi` no lock;
- se a versão do `objc2` está entre as afetadas pelo #861 e se há release corrigida. Não trocar a versão sem o usuário decidir;
- o tempo do build release limpo, o tamanho do binário e o número de crates;
- se o `test-support` funcionou;
- se o texto apareceu na janela.

- [ ] **Step 11: Sabotagem do gate e gate completo**

- Criar `rust/sabotagem.txt` (Write, com qualquer texto) e rodar `make gpui-test`. Tem de reprovar com "há arquivos novos ou alterados em rust/". Depois, `rm rust/sabotagem.txt`.
- Trocar com Edit a versão do `uniffi` em `apps/gpui/Cargo.lock` por `0.0.0` e rodar `apps/gpui/tools/check_core_untouched.sh`. Tem de reprovar. Desfazer com Edit.

Run: `make gpui-fmt && make gpui-test && make test`
Expected: tudo verde.

- [ ] **Step 12: Ponto de parada**

Mostrar ao usuário o que o Step 10 registrou e o `git status --short`. Commit só se ele pedir, com `build(gpui): criar workspace do prototipo com GPUI do Zed v1.21.0`. O lockfile tem milhares de linhas, mas é um arquivo só.

### Task 1: Tipos da fronteira e portes puros (formato, tela, layout)

**Files:**
- Create: `apps/gpui/model/src/port.rs`
- Create: `apps/gpui/model/src/format.rs`
- Create: `apps/gpui/model/src/display.rs`
- Create: `apps/gpui/model/src/layout.rs`
- Create: `apps/gpui/model/src/media/mod.rs`
- Create: `apps/gpui/model/src/metadata.rs`
- Modify: `apps/gpui/model/src/lib.rs`
- Create: `apps/gpui/model/tests/support/mod.rs`
- Test: `apps/gpui/model/tests/format.rs`, `apps/gpui/model/tests/display.rs`, `apps/gpui/model/tests/layout.rs`

**Interfaces:**
- Consumes: `hog_audio::api::{HogPlayer, PlayerError, QueueItem, Snapshot, TrackFormat, TransitionTimings}`, `hog_audio::transitions::PlayerState` (core, sem mudança).
- Produces:
  - `port::Player` (trait), `port::Track { id: u64, path: String, failure: Option<String> }`, `port::Format { sample_rate: f64, bit_depth: u32, codec: String }`, `port::Timings` (7 campos `f64`, `Copy`);
  - `format::{format_time(f64) -> String, quality_detail(f64, u32, &str) -> String, compact_quality(f64, u32) -> String}`;
  - `display::{DisplayState, ClockState, display_state(&Snapshot, Option<&TrackMetadata>, Option<&Format>) -> DisplayState, clock_state(&Snapshot) -> ClockState, NO_TRACK_TITLE}`;
  - `layout::{cover_side(f32, f32) -> f32, PADDING, COLUMN_SPACING, CONTROLS_HEIGHT, MIN_QUEUE_WIDTH, COVER_MIN, COVER_MAX, MIN_WINDOW, DEFAULT_WINDOW}`;
  - `media::{Raster, Artwork, ArtworkRef, Thumbnail}`, `metadata::TrackMetadata`;
  - `tests/support`: `fixture(&str) -> String`, `snapshot(PlayerState, f64, u64, Option<u32>, u32) -> Snapshot`.

- [ ] **Step 1: Escrever o suporte de teste e os testes (vermelho)**

`apps/gpui/model/tests/support/mod.rs`:

```rust
#![allow(dead_code)]

use hog_audio::api::Snapshot;
use hog_audio::transitions::PlayerState;

/// Caminho de uma fixture de `testdata/` na raiz do repositório. Reprova se ela não existir:
/// um teste que depende de fixture não pode passar sem exercitá-la.
pub fn fixture(name: &str) -> String {
    let path = format!("{}/../../../testdata/{name}", env!("CARGO_MANIFEST_DIR"));
    assert!(
        std::path::Path::new(&path).exists(),
        "fixture ausente: {path}\nrode `make fixtures` na raiz do repositório"
    );
    path
}

pub fn snapshot(
    state: PlayerState,
    elapsed: f64,
    underruns: u64,
    current: Option<u32>,
    queue_len: u32,
) -> Snapshot {
    Snapshot {
        state,
        elapsed_seconds: elapsed,
        total_seconds: 208.7,
        underruns,
        volume_scalar: Some(0.5),
        muted: false,
        current_index: current,
        queue_len,
        queue_version: 0,
        device_suspect: false,
        problem_seq: 0,
    }
}
```

`apps/gpui/model/tests/format.rs`:

```rust
use hog_player_model::format::{compact_quality, format_time, quality_detail};

#[test]
fn formata_segundos_como_relogio() {
    assert_eq!(format_time(0.0), "0:00");
    assert_eq!(format_time(7.0), "0:07");
    assert_eq!(format_time(83.0), "1:23");
    assert_eq!(format_time(3599.0), "59:59");
    assert_eq!(format_time(3600.0), "1:00:00");
    assert_eq!(format_time(3723.0), "1:02:03");
}

#[test]
fn tempo_invalido_nao_quebra_o_relogio() {
    assert_eq!(format_time(-5.0), "0:00");
    assert_eq!(format_time(f64::NAN), "0:00");
    assert_eq!(format_time(f64::INFINITY), "0:00");
}

#[test]
fn selo_compacto_usa_bits_barra_kilohertz() {
    assert_eq!(compact_quality(44_100.0, 24), "24/44.1");
    assert_eq!(compact_quality(96_000.0, 24), "24/96");
    assert_eq!(compact_quality(192_000.0, 24), "24/192");
    assert_eq!(compact_quality(44_100.0, 16), "16/44.1");
}

#[test]
fn detalhe_de_qualidade_mostra_a_casa_decimal_so_quando_existe() {
    assert_eq!(quality_detail(44_100.0, 24, "flac"), "44.1 kHz · 24 bit · flac");
    assert_eq!(quality_detail(96_000.0, 24, "flac"), "96 kHz · 24 bit · flac");
}
```

`apps/gpui/model/tests/display.rs`:

```rust
mod support;

use hog_audio::transitions::PlayerState;
use hog_player_model::display::{NO_TRACK_TITLE, clock_state, display_state};
use hog_player_model::metadata::TrackMetadata;
use hog_player_model::port::Format;
use support::snapshot;

fn faixa() -> Format {
    Format { sample_rate: 96_000.0, bit_depth: 24, codec: "flac".to_string() }
}

fn tags() -> TrackMetadata {
    TrackMetadata {
        title: "House of Memories".to_string(),
        artist: Some("Panic! At The Disco".to_string()),
        album: Some("Death of a Bachelor".to_string()),
        artwork: None,
    }
}

#[test]
fn tocando_mostra_pausar() {
    let d = display_state(&snapshot(PlayerState::Playing, 83.0, 0, Some(0), 1), Some(&tags()), Some(&faixa()));
    assert!(d.is_playing);
    assert!(d.can_pause);
    assert_eq!(d.title, "House of Memories");
}

#[test]
fn o_relogio_mostra_o_tempo_e_o_progresso() {
    let c = clock_state(&snapshot(PlayerState::Playing, 83.0, 0, Some(0), 1));
    assert_eq!(c.elapsed, "1:23");
    assert_eq!(c.total, "3:28");
    assert!((c.progress - 83.0 / 208.7).abs() < 0.001);
}

#[test]
fn sem_faixa_carregada_nao_da_para_tocar() {
    let mut vazio = snapshot(PlayerState::Idle, 0.0, 0, None, 0);
    vazio.total_seconds = 0.0;
    let d = display_state(&vazio, None, None);
    assert!(!d.can_play);
    assert!(!d.can_pause);
    assert_eq!(d.title, NO_TRACK_TITLE);
    assert_eq!(clock_state(&vazio).progress, 0.0);
    assert!(d.technical_line.is_empty());
}

#[test]
fn a_linha_tecnica_mostra_o_que_o_projeto_faz_de_diferente() {
    let d = display_state(&snapshot(PlayerState::Playing, 0.0, 0, Some(0), 1), Some(&tags()), Some(&faixa()));
    assert!(d.technical_line.contains("96 kHz"));
    assert!(d.technical_line.contains("24 bit"));
    assert!(d.technical_line.contains("flac"));
    assert!(d.technical_line.contains("hog ativo"));
}

#[test]
fn pausado_ainda_segura_o_device() {
    let d = display_state(&snapshot(PlayerState::Paused, 40.0, 0, Some(0), 1), Some(&tags()), Some(&faixa()));
    assert!(!d.is_playing);
    assert!(d.can_play);
    assert!(d.technical_line.contains("hog ativo"));
}

#[test]
fn carregado_ainda_nao_segura_o_device() {
    let d = display_state(&snapshot(PlayerState::Loaded, 0.0, 0, Some(0), 1), Some(&tags()), Some(&faixa()));
    assert!(d.can_play);
    assert!(!d.technical_line.contains("hog ativo"));
}

#[test]
fn underruns_aparecem_na_linha_tecnica() {
    let d = display_state(&snapshot(PlayerState::Playing, 0.0, 3, Some(0), 1), Some(&tags()), Some(&faixa()));
    assert!(d.technical_line.contains("3 underruns"));
}

#[test]
fn taxa_derivada_de_cd_mostra_a_casa_decimal() {
    let cd = Format { sample_rate: 44_100.0, bit_depth: 16, codec: "flac".to_string() };
    let d = display_state(&snapshot(PlayerState::Playing, 0.0, 0, Some(0), 1), Some(&tags()), Some(&cd));
    assert!(d.technical_line.contains("44.1 kHz"));
}

#[test]
fn a_faixa_terminada_pode_ser_tocada_de_novo() {
    let d = display_state(&snapshot(PlayerState::Finished, 208.7, 0, Some(0), 1), Some(&tags()), Some(&faixa()));
    assert!(d.can_play);
    assert!(!d.can_pause);
    assert!(d.technical_line.contains("hog ativo"));
}

#[test]
fn navegacao_respeita_pontas_e_regra_dos_tres_segundos() {
    let first = display_state(&snapshot(PlayerState::Playing, 2.9, 0, Some(0), 3), Some(&tags()), Some(&faixa()));
    assert!(!first.can_go_previous);
    assert!(first.can_go_next);

    let rewind = display_state(&snapshot(PlayerState::Playing, 3.0, 0, Some(0), 3), Some(&tags()), Some(&faixa()));
    assert!(rewind.can_go_previous);

    let last = display_state(&snapshot(PlayerState::Paused, 0.0, 0, Some(2), 3), Some(&tags()), Some(&faixa()));
    assert!(last.can_go_previous);
    assert!(!last.can_go_next);
}
```

`apps/gpui/model/tests/layout.rs`:

```rust
use hog_player_model::layout::{
    COLUMN_SPACING, CONTROLS_HEIGHT, COVER_MAX, COVER_MIN, MIN_QUEUE_WIDTH, MIN_WINDOW, PADDING,
    cover_side,
};

fn queue_width(width: f32, cover: f32) -> f32 {
    width - 2.0 * PADDING - COLUMN_SPACING - cover
}

#[test]
fn no_tamanho_minimo_capa_e_fila_cabem() {
    let (width, height) = MIN_WINDOW;
    let cover = cover_side(width, height);
    assert!(cover >= COVER_MIN);
    assert!(queue_width(width, cover) >= MIN_QUEUE_WIDTH);
    assert!(cover + CONTROLS_HEIGHT + 2.0 * PADDING <= height);
}

#[test]
fn com_largura_sobrando_a_capa_cresce_com_a_altura() {
    let baixa = cover_side(1400.0, 700.0);
    let alta = cover_side(1400.0, 800.0);
    assert_eq!(alta - baixa, 100.0);
}

#[test]
fn largura_curta_encolhe_a_capa_para_preservar_a_fila() {
    let cover = cover_side(820.0, 1000.0);
    assert_eq!(queue_width(820.0, cover), MIN_QUEUE_WIDTH);
}

#[test]
fn capa_nunca_passa_do_teto() {
    assert_eq!(cover_side(3000.0, 3000.0), COVER_MAX);
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cd apps/gpui && cargo test -p hog-player-model --test format --test display --test layout`
Expected: falha de compilação com `unresolved import hog_player_model::format` (e `display`, `layout`, `metadata`, `port`).

- [ ] **Step 3: Implementar**

`apps/gpui/model/src/lib.rs`:

```rust
//! O player sem interface: a fronteira com o core, os estados que a tela mostra e o
//! controlador que porta as regras do `PlayerViewModel.swift`. Nada aqui depende de GPUI.

pub mod display;
pub mod format;
pub mod layout;
pub mod media;
pub mod metadata;
pub mod port;
```

`apps/gpui/model/src/port.rs`:

```rust
//! A fronteira com o core. A trait existe para o controlador ser testado com um player falso;
//! a implementação para o `HogPlayer` só repassa, sem regra nenhuma.

use hog_audio::api::{
    HogPlayer, PlayerError, QueueItem, Snapshot, TrackFormat, TransitionTimings,
};

pub trait Player: Send + Sync + 'static {
    fn append_tracks(&self, paths: Vec<String>) -> u32;
    fn queue_items(&self) -> Vec<QueueItem>;
    fn try_queue_items(&self) -> Option<Vec<QueueItem>>;
    fn navigate(&self, steps: i32) -> Result<TrackFormat, PlayerError>;
    fn select_track(&self, index: u32) -> Result<TrackFormat, PlayerError>;
    fn select_track_id(&self, id: u64) -> Result<TrackFormat, PlayerError>;
    fn advance(&self) -> Result<Option<TrackFormat>, PlayerError>;
    fn remove_track_id(&self, id: u64) -> Result<Option<TrackFormat>, PlayerError>;
    fn clear_queue(&self) -> Result<(), PlayerError>;
    fn play(&self) -> Result<(), PlayerError>;
    fn pause(&self) -> Result<(), PlayerError>;
    fn set_volume(&self, scalar: f32) -> Result<(), PlayerError>;
    fn snapshot(&self) -> Snapshot;
    fn recover(&self) -> Result<bool, PlayerError>;
    fn last_problem(&self) -> Option<String>;
    fn last_timings(&self) -> TransitionTimings;
    fn shutdown(&self) -> Result<(), PlayerError>;
}

impl Player for HogPlayer {
    fn append_tracks(&self, paths: Vec<String>) -> u32 {
        HogPlayer::append_tracks(self, paths)
    }
    fn queue_items(&self) -> Vec<QueueItem> {
        HogPlayer::queue_items(self)
    }
    fn try_queue_items(&self) -> Option<Vec<QueueItem>> {
        HogPlayer::try_queue_items(self)
    }
    fn navigate(&self, steps: i32) -> Result<TrackFormat, PlayerError> {
        HogPlayer::navigate(self, steps)
    }
    fn select_track(&self, index: u32) -> Result<TrackFormat, PlayerError> {
        HogPlayer::select_track(self, index)
    }
    fn select_track_id(&self, id: u64) -> Result<TrackFormat, PlayerError> {
        HogPlayer::select_track_id(self, id)
    }
    fn advance(&self) -> Result<Option<TrackFormat>, PlayerError> {
        HogPlayer::advance(self)
    }
    fn remove_track_id(&self, id: u64) -> Result<Option<TrackFormat>, PlayerError> {
        HogPlayer::remove_track_id(self, id)
    }
    fn clear_queue(&self) -> Result<(), PlayerError> {
        HogPlayer::clear_queue(self)
    }
    fn play(&self) -> Result<(), PlayerError> {
        HogPlayer::play(self)
    }
    fn pause(&self) -> Result<(), PlayerError> {
        HogPlayer::pause(self)
    }
    fn set_volume(&self, scalar: f32) -> Result<(), PlayerError> {
        HogPlayer::set_volume(self, scalar)
    }
    fn snapshot(&self) -> Snapshot {
        HogPlayer::snapshot(self)
    }
    fn recover(&self) -> Result<bool, PlayerError> {
        HogPlayer::recover(self)
    }
    fn last_problem(&self) -> Option<String> {
        HogPlayer::last_problem(self)
    }
    fn last_timings(&self) -> TransitionTimings {
        HogPlayer::last_timings(self)
    }
    fn shutdown(&self) -> Result<(), PlayerError> {
        HogPlayer::shutdown(self)
    }
}

/// Uma linha da fila. Os tipos do core não derivam `Clone` nem `PartialEq` (são registros do
/// uniffi), e o modelo precisa dos dois para comparar e para entregar à UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub id: u64,
    pub path: String,
    pub failure: Option<String>,
}

impl From<QueueItem> for Track {
    fn from(item: QueueItem) -> Self {
        Self { id: item.id, path: item.path, failure: item.failure }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Format {
    pub sample_rate: f64,
    pub bit_depth: u32,
    pub codec: String,
}

impl From<&TrackFormat> for Format {
    fn from(format: &TrackFormat) -> Self {
        Self {
            sample_rate: format.sample_rate,
            bit_depth: format.bit_depth,
            codec: format.codec.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Timings {
    pub open_ms: f64,
    pub release_ms: f64,
    pub reopen_ms: f64,
    pub acquire_ms: f64,
    pub volume_ms: f64,
    pub prefill_ms: f64,
    pub start_ms: f64,
}

impl From<TransitionTimings> for Timings {
    fn from(t: TransitionTimings) -> Self {
        Self {
            open_ms: t.open_ms,
            release_ms: t.release_ms,
            reopen_ms: t.reopen_ms,
            acquire_ms: t.acquire_ms,
            volume_ms: t.volume_ms,
            prefill_ms: t.prefill_ms,
            start_ms: t.start_ms,
        }
    }
}
```

`apps/gpui/model/src/format.rs`:

```rust
//! Relógio e qualidade: porte de `TimeFormat.swift` e `Quality.swift`. A linha técnica e o
//! tooltip do selo saem daqui, para nunca descreverem a mesma faixa de jeitos diferentes.

/// Negativo, NaN e infinito viram `0:00`: `total_seconds` chega zerado antes da carga, e taxa
/// zerada produz NaN.
pub fn format_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "0:00".to_string();
    }
    let total = seconds.floor() as u64;
    let (hours, minutes, secs) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

/// 44,1/88,2/176,4 kHz têm casa decimal real; 48/96/192 não. Suprimir o zero evita "96.0" sem
/// arredondar 44,1 para 44, que seria informação errada sobre o que chega ao DAC.
fn format_kilohertz(hz: f64) -> String {
    let khz = hz / 1000.0;
    if khz.fract() == 0.0 {
        format!("{khz:.0}")
    } else {
        format!("{khz:.1}")
    }
}

pub fn quality_detail(sample_rate: f64, bit_depth: u32, codec: &str) -> String {
    format!("{} kHz · {bit_depth} bit · {codec}", format_kilohertz(sample_rate))
}

pub fn compact_quality(sample_rate: f64, bit_depth: u32) -> String {
    format!("{bit_depth}/{}", format_kilohertz(sample_rate))
}
```

`apps/gpui/model/src/media/mod.rs`:

```rust
//! Imagens já decodificadas, prontas para a UI enviar à GPU sem trabalho na main thread.

use std::sync::Arc;

/// Pixels em BGRA, a ordem que o `RenderImage` do GPUI envia (conferida na Task 0).
#[derive(Debug, PartialEq, Eq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// A capa da faixa atual e o fundo já desfocado, gerados uma vez por faixa fora da main.
#[derive(Debug)]
pub struct Artwork {
    pub cover: Raster,
    pub backdrop: Option<Raster>,
}

/// Comparados por ponteiro: o tique compara o modelo dez vezes por segundo, e comparar pixels
/// seria caro. Uma capa nova sempre chega num `Arc` novo.
#[derive(Clone, Debug)]
pub struct ArtworkRef(pub Arc<Artwork>);

impl PartialEq for ArtworkRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Debug)]
pub struct Thumbnail(pub Arc<Raster>);

impl PartialEq for Thumbnail {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
```

`apps/gpui/model/src/metadata.rs`:

```rust
//! O que a tela mostra da faixa atual. A leitura vem na Task 3.

use crate::media::ArtworkRef;

#[derive(Clone, Debug, PartialEq)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork: Option<ArtworkRef>,
}
```

`apps/gpui/model/src/display.rs`:

```rust
//! O que a tela mostra, traduzido do `Snapshot`: porte de `DisplayState.swift`. Funções puras,
//! testáveis sem hardware. O relógio fica à parte porque muda a cada tique.

use hog_audio::api::Snapshot;
use hog_audio::transitions::PlayerState;

use crate::format::{format_time, quality_detail};
use crate::media::ArtworkRef;
use crate::metadata::TrackMetadata;
use crate::port::Format;

pub const NO_TRACK_TITLE: &str = "nenhuma faixa carregada";

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayState {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Option<ArtworkRef>,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub is_playing: bool,
    pub technical_line: String,
}

pub fn display_state(
    snapshot: &Snapshot,
    metadata: Option<&TrackMetadata>,
    format: Option<&Format>,
) -> DisplayState {
    const REWIND_THRESHOLD_SECONDS: f64 = 3.0;
    let playing = snapshot.state == PlayerState::Playing;
    // Em Loaded o hog ainda não foi tomado; em Finished o device continua retido, e é quando
    // "por que o Mac segue mudo" mais surpreende.
    let holds_device = matches!(
        snapshot.state,
        PlayerState::Playing | PlayerState::Paused | PlayerState::Finished
    );
    let technical_line = format
        .map(|format| {
            let mut parts = vec![quality_detail(format.sample_rate, format.bit_depth, &format.codec)];
            if holds_device {
                parts.push("hog ativo".to_string());
            }
            if snapshot.underruns > 0 {
                parts.push(format!("{} underruns", snapshot.underruns));
            }
            parts.join(" · ")
        })
        .unwrap_or_default();

    DisplayState {
        title: metadata.map_or_else(|| NO_TRACK_TITLE.to_string(), |m| m.title.clone()),
        artist: metadata.and_then(|m| m.artist.clone()).unwrap_or_default(),
        album: metadata.and_then(|m| m.album.clone()).unwrap_or_default(),
        artwork: metadata.and_then(|m| m.artwork.clone()),
        can_play: matches!(
            snapshot.state,
            PlayerState::Loaded | PlayerState::Paused | PlayerState::Finished
        ),
        can_pause: playing,
        can_go_next: snapshot.current_index.is_some_and(|i| i + 1 < snapshot.queue_len),
        can_go_previous: snapshot
            .current_index
            .is_some_and(|i| i > 0 || snapshot.elapsed_seconds >= REWIND_THRESHOLD_SECONDS),
        is_playing: playing,
        technical_line,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClockState {
    pub elapsed: String,
    pub total: String,
    pub progress: f64,
}

pub fn clock_state(snapshot: &Snapshot) -> ClockState {
    let progress = if snapshot.total_seconds > 0.0 {
        (snapshot.elapsed_seconds / snapshot.total_seconds).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ClockState {
        elapsed: format_time(snapshot.elapsed_seconds),
        total: format_time(snapshot.total_seconds),
        progress,
    }
}
```

`apps/gpui/model/src/layout.rs`:

```rust
//! A conta de tamanho da janela, porte de `PlayerLayout.swift`: capa quadrada à esquerda,
//! controles de altura fixa abaixo dela e a fila com o que sobrar.

pub const PADDING: f32 = 24.0;
pub const COLUMN_SPACING: f32 = 24.0;
/// A altura medida na janela SwiftUI (261 pt com artista, álbum e linha técnica) mais 3 de
/// folga. A coluna GPUI é montada para caber nela; a Task 9 confere.
pub const CONTROLS_HEIGHT: f32 = 264.0;
pub const MIN_QUEUE_WIDTH: f32 = 340.0;
pub const COVER_MIN: f32 = 320.0;
pub const COVER_MAX: f32 = 560.0;
pub const MIN_WINDOW: (f32, f32) = (820.0, 640.0);
pub const DEFAULT_WINDOW: (f32, f32) = (920.0, 700.0);

/// Lado da capa: cresce com a altura sem roubar a largura mínima da fila.
pub fn cover_side(width: f32, height: f32) -> f32 {
    let by_height = height - 2.0 * PADDING - CONTROLS_HEIGHT;
    let by_width = width - 2.0 * PADDING - COLUMN_SPACING - MIN_QUEUE_WIDTH;
    by_height.min(by_width).clamp(COVER_MIN, COVER_MAX)
}
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cd apps/gpui && cargo test -p hog-player-model --test format --test display --test layout`
Expected: 18 testes passando.

- [ ] **Step 5: Sabotagem**

Trocar em `format_kilohertz` o `if khz.fract() == 0.0` por `if true`, rodar `cargo test -p hog-player-model --test format` e ver `selo_compacto_usa_bits_barra_kilohertz` e `detalhe_de_qualidade_mostra_a_casa_decimal_so_quando_existe` reprovarem com "24/44" no lugar de "24/44.1". Desfazer com Edit (nunca `git checkout`, que perderia o trabalho não commitado).

Depois, trocar `snapshot.elapsed_seconds >= REWIND_THRESHOLD_SECONDS` por `>` e ver `navegacao_respeita_pontas_e_regra_dos_tres_segundos` reprovar. Desfazer.

- [ ] **Step 6: Gate**

Run: `make gpui-test && make test`
Expected: os dois verdes. O `check_core_untouched.sh` imprime "core intocado".

- [ ] **Step 7: Ponto de parada**

Mostrar `git status --short` e o resumo da tarefa ao usuário. Commit só se ele pedir, com `feat(gpui): portar estados de tela, formato e layout`.

### Task 2: Tags e imagens (lofty e image)

**Files:**
- Create: `apps/gpui/model/src/media/tags.rs`
- Create: `apps/gpui/model/src/media/artwork.rs`
- Modify: `apps/gpui/model/src/media/mod.rs` (declarar os submódulos)
- Test: `apps/gpui/model/tests/media.rs`

**Interfaces:**
- Consumes: `media::{Raster, Artwork}` (Task 1); fixtures `meta_fixture.flac`, `meta_fixture_sem_capa.flac`, `meta_fixture.m4a` e `meta_fixture.mp3`, geradas por `make fixtures`.
- Produces:
  - `media::tags::{Tags { title: String, artist: Option<String>, album: Option<String>, artwork: Option<Vec<u8>> }, read_tags(&str) -> Tags, clean_text(Option<Cow<str>>) -> Option<String>, file_stem(&str) -> String}`;
  - `media::artwork::{THUMBNAIL_PIXELS (96), COVER_PIXELS (1200), BACKDROP_PIXELS (160), thumbnail(&[u8], u32) -> Option<Raster>, backdrop(&[u8]) -> Option<Raster>, artwork(&[u8]) -> Option<Artwork>}`.

- [ ] **Step 1: Escrever os testes (vermelho)**

`apps/gpui/model/tests/media.rs`:

```rust
mod support;

use std::borrow::Cow;

use hog_player_model::media::artwork::{
    BACKDROP_PIXELS, COVER_PIXELS, THUMBNAIL_PIXELS, artwork, backdrop, thumbnail,
};
use hog_player_model::media::tags::{clean_text, read_tags};
use support::fixture;

fn png(pixels: image::RgbImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(pixels)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("png");
    bytes
}

fn solid(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    png(image::RgbImage::from_pixel(width, height, image::Rgb(rgb)))
}

#[test]
fn tags_do_flac_com_capa() {
    let tags = read_tags(&fixture("meta_fixture.flac"));
    assert_eq!(tags.title, "Titulo Teste");
    assert_eq!(tags.artist.as_deref(), Some("Artista Teste"));
    assert_eq!(tags.album.as_deref(), Some("Album Teste"));
    assert!(tags.artwork.is_some(), "a capa do FLAC vem de METADATA_BLOCK_PICTURE");
}

#[test]
fn flac_sem_capa_tem_tags_e_nao_tem_capa() {
    let tags = read_tags(&fixture("meta_fixture_sem_capa.flac"));
    assert_eq!(tags.title, "Titulo Teste");
    assert!(tags.artwork.is_none());
}

#[test]
fn m4a_e_mp3_trazem_tags_e_capa() {
    for name in ["meta_fixture.m4a", "meta_fixture.mp3"] {
        let tags = read_tags(&fixture(name));
        assert_eq!(tags.title, "Titulo Teste", "{name}");
        assert!(tags.artwork.is_some(), "{name}");
    }
}

#[test]
fn arquivo_que_nao_abre_vira_o_nome_do_arquivo() {
    let tags = read_tags("/tmp/hog-audio-nao-existe-tags.flac");
    assert_eq!(tags.title, "hog-audio-nao-existe-tags");
    assert!(tags.artist.is_none());
    assert!(tags.album.is_none());
    assert!(tags.artwork.is_none());
}

#[test]
fn texto_vazio_ou_so_espacos_nao_vale() {
    assert_eq!(clean_text(Some(Cow::from("   "))), None);
    assert_eq!(clean_text(Some(Cow::from(" Coldplay \n"))), Some("Coldplay".to_string()));
    assert_eq!(clean_text(None), None);
}

#[test]
fn miniatura_cabe_no_tamanho_pedido_e_mantem_a_proporcao() {
    let raster = thumbnail(&solid(3000, 1000, [10, 120, 200]), THUMBNAIL_PIXELS).expect("miniatura");
    assert_eq!((raster.width, raster.height), (96, 32));
    assert_eq!(raster.bgra.len(), 96 * 32 * 4);
}

#[test]
fn miniatura_nao_amplia_imagem_pequena() {
    let raster = thumbnail(&solid(50, 40, [1, 2, 3]), THUMBNAIL_PIXELS).expect("miniatura");
    assert_eq!((raster.width, raster.height), (50, 40));
}

#[test]
fn capa_enorme_vira_no_maximo_1200() {
    let art = artwork(&solid(3000, 3000, [200, 10, 10])).expect("capa");
    assert_eq!((art.cover.width, art.cover.height), (COVER_PIXELS, COVER_PIXELS));
    assert!(art.backdrop.is_some());
}

#[test]
fn bytes_que_nao_sao_imagem_nao_viram_capa() {
    let lixo = b"nao e imagem";
    assert!(thumbnail(lixo, THUMBNAIL_PIXELS).is_none());
    assert!(backdrop(lixo).is_none());
    assert!(artwork(lixo).is_none());
}

#[test]
fn os_canais_saem_na_ordem_bgra() {
    let raster = thumbnail(&solid(8, 8, [255, 0, 0]), THUMBNAIL_PIXELS).expect("miniatura");
    assert_eq!(&raster.bgra[..4], &[0, 0, 255, 255]);
}

#[test]
fn o_fundo_tem_tamanho_fixo() {
    let raster = backdrop(&solid(640, 120, [10, 120, 200])).expect("fundo");
    assert_eq!((raster.width, raster.height), (BACKDROP_PIXELS, BACKDROP_PIXELS));
}

#[test]
fn o_fundo_desfoca_uma_borda_dura() {
    let mut split = image::RgbImage::new(128, 128);
    for (x, _, pixel) in split.enumerate_pixels_mut() {
        *pixel = if x < 64 { image::Rgb([0, 0, 0]) } else { image::Rgb([255, 255, 255]) };
    }
    let raster = backdrop(&png(split)).expect("fundo");
    let row = (BACKDROP_PIXELS / 2) as usize * BACKDROP_PIXELS as usize * 4;
    let seam = raster.bgra[row + (BACKDROP_PIXELS / 2) as usize * 4];
    assert!((60..=195).contains(&seam), "a borda {seam} devia ter virado gradiente");
}

#[test]
fn a_capa_real_da_fixture_vira_miniatura() {
    let bytes = read_tags(&fixture("meta_fixture.flac")).artwork.expect("capa");
    let raster = thumbnail(&bytes, THUMBNAIL_PIXELS).expect("miniatura");
    assert!(raster.width.max(raster.height) <= THUMBNAIL_PIXELS);
    assert!(raster.width > 0 && raster.height > 0);
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cd apps/gpui && cargo test -p hog-player-model --test media`
Expected: falha de compilação com `unresolved import hog_player_model::media::artwork`.

- [ ] **Step 3: Implementar**

`apps/gpui/model/src/media/mod.rs`: acrescentar no topo, logo depois do comentário do módulo:

```rust
pub mod artwork;
pub mod tags;
```

`apps/gpui/model/src/media/tags.rs`:

```rust
//! Título, artista, álbum e capa pelo lofty. No SwiftUI isso vem do AVFoundation; o core não lê
//! tags. O lofty só lê metadados: decodificar áudio continua com o `ExtAudioFile` do core.

use std::borrow::Cow;
use std::path::Path;

use lofty::picture::PictureType;
use lofty::prelude::{Accessor, TaggedFileExt};

#[derive(Clone, Debug, PartialEq)]
pub struct Tags {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Os bytes da capa como vieram do arquivo (JPEG ou PNG), sem decodificar.
    pub artwork: Option<Vec<u8>>,
}

impl Tags {
    fn only_title(title: String) -> Self {
        Self { title, artist: None, album: None, artwork: None }
    }
}

/// Nunca falha: sem tags legíveis, o título vira o nome do arquivo, como no SwiftUI.
pub fn read_tags(path: &str) -> Tags {
    let fallback = file_stem(path);
    let Ok(tagged) = lofty::read_from_path(path) else {
        return Tags::only_title(fallback);
    };
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Tags::only_title(fallback);
    };
    let pictures = tag.pictures();
    let artwork = pictures
        .iter()
        .find(|picture| picture.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())
        .map(|picture| picture.data().to_vec());
    Tags {
        title: clean_text(tag.title()).unwrap_or(fallback),
        artist: clean_text(tag.artist()),
        album: clean_text(tag.album()),
        artwork,
    }
}

/// Tag vazia ou só com espaços conta como ausente.
pub fn clean_text(value: Option<Cow<'_, str>>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

pub fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}
```

`apps/gpui/model/src/media/artwork.rs`:

```rust
//! Miniatura, capa e fundo desfocado a partir dos bytes da capa embutida. Tudo roda fora da
//! main thread e sai em BGRA, pronto para o GPUI enviar à GPU. A técnica (desfocar na CPU uma
//! versão pequena) vem do Pawse; os números vêm do SwiftUI, para a comparação medir a
//! tecnologia e não um visual diferente.

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};

use super::{Artwork, Raster};

/// 44 pt em Retina são 88 px. A fila guarda só isto: a capa inteira tem de 1 a 5 MB.
pub const THUMBNAIL_PIXELS: u32 = 96;
/// A coluna tem no máximo 560 pt, 1120 px em Retina.
pub const COVER_PIXELS: u32 = 1200;
/// O fundo não precisa de detalhe: reduzir antes de desfocar deixa o blur barato.
pub const BACKDROP_PIXELS: u32 = 160;

pub fn thumbnail(bytes: &[u8], max_pixels: u32) -> Option<Raster> {
    let image = image::load_from_memory(bytes).ok()?;
    Some(to_bgra(&fit(image, max_pixels)))
}

pub fn backdrop(bytes: &[u8]) -> Option<Raster> {
    let image = image::load_from_memory(bytes).ok()?;
    Some(blurred(&image))
}

/// Decodifica uma vez só e gera a capa e o fundo da faixa atual.
pub fn artwork(bytes: &[u8]) -> Option<Artwork> {
    let image = image::load_from_memory(bytes).ok()?;
    let backdrop = blurred(&image);
    Some(Artwork {
        cover: to_bgra(&fit(image, COVER_PIXELS)),
        backdrop: Some(backdrop),
    })
}

/// Reduz para caber em `max_pixels` no lado maior. Imagem menor fica como está.
fn fit(image: DynamicImage, max_pixels: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width.max(height) <= max_pixels {
        image
    } else {
        image.resize(max_pixels, max_pixels, FilterType::Triangle)
    }
}

fn blurred(image: &DynamicImage) -> Raster {
    const SIGMA: f32 = 10.0;
    let small = image.resize_to_fill(BACKDROP_PIXELS, BACKDROP_PIXELS, FilterType::Triangle);
    let blurred = image::imageops::fast_blur(&small.to_rgba8(), SIGMA);
    to_bgra(&DynamicImage::ImageRgba8(blurred))
}

fn to_bgra(image: &DynamicImage) -> Raster {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut bgra = rgba.into_raw();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Raster { width, height, bgra }
}
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cd apps/gpui && cargo test -p hog-player-model --test media`
Expected: 13 testes passando. Se `tags_do_flac_com_capa` reprovar só na capa, conferir no código do lofty 0.25.4 (`src/ogg/tag/read.rs:105`) se o `METADATA_BLOCK_PICTURE` do Vorbis comment chega ao `Tag` genérico do FLAC. Se não chegar, ler as imagens pelo `FlacFile` (`lofty::flac::FlacFile::pictures`) e registrar a diferença no resultado. Não mexer na fixture.

- [ ] **Step 5: Sabotagem**

- Tirar o `pixel.swap(0, 2)`. `os_canais_saem_na_ordem_bgra` tem de reprovar. Desfazer.
- Trocar em `fit` o `<=` por `<` e o `image.resize` por `image.resize_exact`. `miniatura_cabe_no_tamanho_pedido_e_mantem_a_proporcao` tem de reprovar com 96×96. Desfazer.
- Tirar o `.filter(|text| !text.is_empty())`. `texto_vazio_ou_so_espacos_nao_vale` tem de reprovar. Desfazer.

- [ ] **Step 6: Gate**

Run: `make gpui-fmt && make gpui-test && make test`
Expected: tudo verde.

- [ ] **Step 7: Ponto de parada**

Mostrar `git status --short` e o resumo. Commit só se o usuário pedir, com `feat(gpui): ler tags e gerar miniatura, capa e fundo`.

---

### Task 3: Linha da fila, metadados da faixa atual e arquivos de áudio

**Files:**
- Create: `apps/gpui/model/src/entry.rs`
- Create: `apps/gpui/model/src/audio_files.rs`
- Modify: `apps/gpui/model/src/metadata.rs`
- Modify: `apps/gpui/model/src/lib.rs`
- Test: `apps/gpui/model/tests/entry.rs`, `apps/gpui/model/tests/audio_files.rs`

**Interfaces:**
- Consumes:
  - de tarefas anteriores: `media::tags::{read_tags, Tags}`, `media::artwork::{thumbnail, artwork, THUMBNAIL_PIXELS}` e `format::*`;
  - do core: `hog_audio::api::probe_track(String) -> Result<TrackProbe, PlayerError>`.
- Produces:
  - `entry::QueueEntryInfo { title, artist: Option<String>, album: Option<String>, quality: Option<String>, quality_detail: Option<String>, duration: Option<String>, thumbnail: Option<Thumbnail>, problem: Option<String> }` com `subtitle() -> String`;
  - `entry::ProbeOutcome::{Probed { sample_rate, bit_depth, codec, total_seconds }, Failed(String)}`;
  - `entry::queue_entry_info(&Tags, ProbeOutcome, Option<Thumbnail>) -> QueueEntryInfo` e `entry::load_queue_entry(&str) -> QueueEntryInfo`;
  - `metadata::load_track_metadata(&str) -> TrackMetadata`;
  - `audio_files::audio_files(&[PathBuf]) -> Vec<PathBuf>` e `audio_files::AUDIO_EXTENSIONS`.

- [ ] **Step 1: Escrever os testes (vermelho)**

`apps/gpui/model/tests/entry.rs`:

```rust
mod support;

use hog_player_model::entry::{ProbeOutcome, load_queue_entry, queue_entry_info};
use hog_player_model::media::tags::Tags;
use hog_player_model::metadata::load_track_metadata;
use support::fixture;

fn tags(artist: Option<&str>, album: Option<&str>) -> Tags {
    Tags {
        title: "Life in Technicolor".to_string(),
        artist: artist.map(str::to_string),
        album: album.map(str::to_string),
        artwork: None,
    }
}

fn sonda() -> ProbeOutcome {
    ProbeOutcome::Probed {
        sample_rate: 44_100.0,
        bit_depth: 24,
        codec: "flac".to_string(),
        total_seconds: 149.6,
    }
}

#[test]
fn subtitulo_junta_artista_e_album_so_quando_existem() {
    let ambos = queue_entry_info(&tags(Some("Coldplay"), Some("Viva La Vida")), sonda(), None);
    assert_eq!(ambos.subtitle(), "Coldplay — Viva La Vida");
    assert_eq!(queue_entry_info(&tags(Some("Coldplay"), None), sonda(), None).subtitle(), "Coldplay");
    assert_eq!(queue_entry_info(&tags(None, Some("Viva La Vida")), sonda(), None).subtitle(), "Viva La Vida");
    assert!(queue_entry_info(&tags(None, None), sonda(), None).subtitle().is_empty());
}

#[test]
fn sondagem_bem_sucedida_preenche_selo_tooltip_e_duracao() {
    let info = queue_entry_info(&tags(Some("Coldplay"), Some("Viva La Vida")), sonda(), None);
    assert_eq!(info.title, "Life in Technicolor");
    assert_eq!(info.quality.as_deref(), Some("24/44.1"));
    assert_eq!(info.quality_detail.as_deref(), Some("44.1 kHz · 24 bit · flac"));
    assert_eq!(info.duration.as_deref(), Some("2:29"));
    assert!(info.problem.is_none());
}

#[test]
fn sondagem_que_falha_vira_problema_sem_qualidade() {
    let info = queue_entry_info(
        &tags(Some("Coldplay"), None),
        ProbeOutcome::Failed("não consegui abrir o arquivo".to_string()),
        None,
    );
    assert_eq!(info.problem.as_deref(), Some("não consegui abrir o arquivo"));
    assert!(info.quality.is_none());
    assert!(info.duration.is_none());
    assert_eq!(info.title, "Life in Technicolor");
}

#[test]
fn carga_real_usa_a_mesma_sondagem_do_engine() {
    let info = load_queue_entry(&fixture("t96_24.flac"));
    assert_eq!(info.quality.as_deref(), Some("24/96"));
    assert_eq!(info.quality_detail.as_deref(), Some("96 kHz · 24 bit · flac"));
    assert_eq!(info.duration.as_deref(), Some("0:06"));
    assert!(info.problem.is_none());
}

#[test]
fn carga_real_com_capa_traz_tags_e_miniatura() {
    let info = load_queue_entry(&fixture("meta_fixture.flac"));
    assert_eq!(info.title, "Titulo Teste");
    assert_eq!(info.subtitle(), "Artista Teste — Album Teste");
    assert!(info.thumbnail.is_some());
}

#[test]
fn arquivo_que_nao_abre_vira_problema_com_a_mensagem_do_core() {
    let info = load_queue_entry("/tmp/hog-audio-nao-existe-fila.flac");
    let problem = info.problem.expect("devia virar problema");
    assert!(!problem.is_empty());
    assert!(!problem.starts_with("Load"), "a mensagem do core, não o Debug do enum: {problem}");
    assert_eq!(info.title, "hog-audio-nao-existe-fila");
}

#[test]
fn metadados_da_faixa_atual_trazem_capa_e_fundo() {
    let meta = load_track_metadata(&fixture("meta_fixture.flac"));
    assert_eq!(meta.title, "Titulo Teste");
    let art = meta.artwork.expect("capa");
    assert!(art.0.cover.width > 0);
    assert!(art.0.backdrop.is_some());
}

#[test]
fn faixa_sem_capa_nao_tem_artwork() {
    assert!(load_track_metadata(&fixture("meta_fixture_sem_capa.flac")).artwork.is_none());
}
```

`apps/gpui/model/tests/audio_files.rs`:

```rust
use std::fs;
use std::path::PathBuf;

use hog_player_model::audio_files::audio_files;

fn pasta(nome: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("hog-gpui-{nome}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("pasta");
    root
}

fn nomes(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn expande_pastas_aninhadas_filtra_e_ordena_audio() {
    let root = pasta("ingestao");
    let nested = root.join("CD2");
    fs::create_dir_all(&nested).unwrap();
    for path in [
        root.join("02-song.wav"),
        root.join("cover.jpg"),
        nested.join("01-song.flac"),
        nested.join("notes.txt"),
    ] {
        fs::write(path, b"").unwrap();
    }

    let result = audio_files(&[root.clone()]);

    assert_eq!(nomes(&result), ["02-song.wav", "01-song.flac"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn lista_vazia_ou_sem_audio_devolve_vazio() {
    let root = pasta("sem-audio");
    fs::write(root.join("cover.png"), b"").unwrap();

    assert!(audio_files(&[]).is_empty());
    assert!(audio_files(&[root.clone()]).is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn extensao_em_maiuscula_vale_e_oculto_nao() {
    let root = pasta("caixa");
    fs::write(root.join("FAIXA.FLAC"), b"").unwrap();
    fs::write(root.join(".oculta.flac"), b"").unwrap();

    assert_eq!(nomes(&audio_files(&[root.clone()])), ["FAIXA.FLAC"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn arquivo_solto_e_caminho_inexistente() {
    let root = pasta("solto");
    let faixa = root.join("a.m4a");
    fs::write(&faixa, b"").unwrap();

    let result = audio_files(&[faixa.clone(), root.join("nao-existe.flac")]);

    assert_eq!(result, [faixa]);
    fs::remove_dir_all(root).unwrap();
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cd apps/gpui && cargo test -p hog-player-model --test entry --test audio_files`
Expected: falha de compilação com `unresolved import hog_player_model::entry` e `hog_player_model::audio_files`.

- [ ] **Step 3: Implementar**

`apps/gpui/model/src/lib.rs`: a lista de módulos passa a ser

```rust
pub mod audio_files;
pub mod display;
pub mod entry;
pub mod format;
pub mod layout;
pub mod media;
pub mod metadata;
pub mod port;
```

`apps/gpui/model/src/entry.rs`:

```rust
//! O que uma linha da fila mostra: porte de `QueueEntry.swift`. Até a carga terminar, a linha
//! usa o nome do arquivo.

use std::sync::Arc;

use crate::format::{compact_quality, format_time, quality_detail};
use crate::media::Thumbnail;
use crate::media::artwork::{THUMBNAIL_PIXELS, thumbnail};
use crate::media::tags::{Tags, read_tags};

#[derive(Clone, Debug, PartialEq)]
pub struct QueueEntryInfo {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub quality: Option<String>,
    pub quality_detail: Option<String>,
    pub duration: Option<String>,
    pub thumbnail: Option<Thumbnail>,
    pub problem: Option<String>,
}

impl QueueEntryInfo {
    pub fn subtitle(&self) -> String {
        [self.artist.as_deref(), self.album.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" — ")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProbeOutcome {
    Probed { sample_rate: f64, bit_depth: u32, codec: String, total_seconds: f64 },
    Failed(String),
}

pub fn queue_entry_info(tags: &Tags, probe: ProbeOutcome, thumbnail: Option<Thumbnail>) -> QueueEntryInfo {
    let (quality, detail, duration, problem) = match probe {
        ProbeOutcome::Probed { sample_rate, bit_depth, codec, total_seconds } => (
            Some(compact_quality(sample_rate, bit_depth)),
            Some(quality_detail(sample_rate, bit_depth, &codec)),
            Some(format_time(total_seconds)),
            None,
        ),
        ProbeOutcome::Failed(reason) => (None, None, None, Some(reason)),
    };
    QueueEntryInfo {
        title: tags.title.clone(),
        artist: tags.artist.clone(),
        album: tags.album.clone(),
        quality,
        quality_detail: detail,
        duration,
        thumbnail,
        problem,
    }
}

/// Tags pelo lofty e formato pelo mesmo `AudioSource` que o engine usa: o selo mostra o que
/// vai chegar ao DAC, não o que a extensão sugere.
pub fn load_queue_entry(path: &str) -> QueueEntryInfo {
    let tags = read_tags(path);
    let probe = match hog_audio::api::probe_track(path.to_string()) {
        Ok(probe) => ProbeOutcome::Probed {
            sample_rate: probe.sample_rate,
            bit_depth: probe.bit_depth,
            codec: probe.codec,
            total_seconds: probe.total_seconds,
        },
        Err(error) => ProbeOutcome::Failed(error.to_string()),
    };
    let thumbnail = tags
        .artwork
        .as_deref()
        .and_then(|bytes| thumbnail(bytes, THUMBNAIL_PIXELS))
        .map(|raster| Thumbnail(Arc::new(raster)));
    queue_entry_info(&tags, probe, thumbnail)
}
```

`apps/gpui/model/src/metadata.rs` (substitui o arquivo inteiro):

```rust
//! O que a tela mostra da faixa atual, com a capa grande e o fundo já prontos.

use std::sync::Arc;

use crate::media::artwork::artwork;
use crate::media::tags::read_tags;
use crate::media::ArtworkRef;

#[derive(Clone, Debug, PartialEq)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork: Option<ArtworkRef>,
}

/// Roda fora da main: decodificar uma capa de 3000 px e desfocar leva dezenas de ms.
pub fn load_track_metadata(path: &str) -> TrackMetadata {
    let tags = read_tags(path);
    let artwork = tags
        .artwork
        .as_deref()
        .and_then(artwork)
        .map(|art| ArtworkRef(Arc::new(art)));
    TrackMetadata { title: tags.title, artist: tags.artist, album: tags.album, artwork }
}
```

`apps/gpui/model/src/audio_files.rs`:

```rust
//! Expande pastas, descarta o que não é áudio e ordena por caminho: porte de `AudioFiles.swift`.
//! Roda na thread de comandos, nunca na main, porque uma pasta grande leva segundos.

use std::fs;
use std::path::{Path, PathBuf};

/// O que o `ExtAudioFile` do core abre. O SwiftUI filtra por `UTType.audio`; aqui a lista é
/// explícita. A diferença é deliberada e fica registrada no resultado.
pub const AUDIO_EXTENSIONS: [&str; 9] =
    ["aac", "aif", "aifc", "aiff", "caf", "flac", "m4a", "mp3", "wav"];

pub fn audio_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for path in paths {
        collect(path, &mut found);
    }
    // Byte a byte, como o `<` de String do Swift: `PathBuf` compara por componente.
    found.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    found
}

fn collect(path: &Path, found: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = fs::read_dir(path) else { return };
        for entry in entries.flatten() {
            let child = entry.path();
            if is_hidden(&child) {
                continue;
            }
            // Pasta por symlink não é seguida: evita ciclo.
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => collect(&child, found),
                Ok(_) if child.is_file() && is_audio(&child) => found.push(child),
                _ => {}
            }
        }
    } else if path.is_file() && is_audio(path) {
        found.push(path.to_path_buf());
    }
}

fn is_hidden(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.as_str()))
}
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cd apps/gpui && cargo test -p hog-player-model --test entry --test audio_files`
Expected: 12 testes passando.

- [ ] **Step 5: Sabotagem**

- Trocar em `load_queue_entry` o `error.to_string()` por `format!("{error:?}")`. `arquivo_que_nao_abre_vira_problema_com_a_mensagem_do_core` tem de reprovar. Desfazer.
- Tirar o `found.sort_by(...)`. `expande_pastas_aninhadas_filtra_e_ordena_audio` tem de reprovar ou ficar instável; rodar 3 vezes. Se passar nas três, o teste não prova a ordenação: acrescentar um terceiro arquivo `00-intro.flac` em `CD2` antes de desfazer, e ajustar o esperado para `["02-song.wav", "00-intro.flac", "01-song.flac"]`.
- Tirar o `.to_ascii_lowercase()`. `extensao_em_maiuscula_vale_e_oculto_nao` tem de reprovar. Desfazer.

- [ ] **Step 6: Gate**

Run: `make gpui-fmt && make gpui-test && make test`
Expected: tudo verde.

- [ ] **Step 7: Ponto de parada**

Mostrar `git status --short` e o resumo. Commit só se o usuário pedir, com `feat(gpui): carregar linha da fila, metadados e arquivos de audio`.

---

### Task 4: Controlador (porte do `PlayerViewModel.swift`)

É a maior tarefa, e de propósito não foi dividida. O `tick()` integra comandos, volume e cargas, e dividir obrigaria a escrever stubs que seriam reescritos na tarefa seguinte. Os testes vêm em quatro arquivos, um por assunto, e são escritos antes do código.

**Files:**
- Create: `apps/gpui/testing/Cargo.toml`, `apps/gpui/testing/src/lib.rs` (o `FakePlayer` e os helpers de teste, num crate só de teste)
- Modify: `apps/gpui/Cargo.toml` (membro `testing`)
- Modify: `apps/gpui/model/Cargo.toml` (`[dev-dependencies]`)
- Create: `apps/gpui/model/src/controller/mod.rs`, `model.rs`, `commands.rs`, `volume.rs`, `loaders.rs`
- Modify: `apps/gpui/model/src/lib.rs` (`pub mod controller;`)
- Test: `apps/gpui/model/tests/controller_transport.rs`, `controller_volume.rs`, `controller_queue.rs`, `controller_observation.rs`

**Interfaces:**
- Consumes (Tasks 1 a 3):
  - `port::{Player, Track, Format, Timings}`;
  - `display::{display_state, clock_state, DisplayState, ClockState, NO_TRACK_TITLE}`;
  - `entry::{QueueEntryInfo, load_queue_entry}`, `metadata::{TrackMetadata, load_track_metadata}`;
  - `media::tags::file_stem`, `audio_files::audio_files`.
- Produces:
  - `controller::Controller` com:
    - `new(Arc<dyn Player>, Config) -> Self` e `model(&self) -> &Model`;
    - `tick(&mut self) -> Changes`;
    - comandos: `toggle()`, `next()`, `previous()`, `select(u64)`, `remove(u64)`, `clear()`, `add(Vec<PathBuf>)`;
    - volume: `apply_volume(f64)` e `set_volume_editing(bool)`;
    - encerramento: `shutdown() -> Result<(), String>`;
    - esperas: `is_idle()`, `wait_idle(Duration) -> bool`, `wait_entries(Duration) -> bool`, `wait_metadata(Duration) -> bool`, `wait_until(Duration, impl Fn(&Controller) -> bool) -> bool`.
  - `controller::Config { load_entry: EntryLoad, load_metadata: MetadataLoad, log: LogSink, wake: Wake }`, com `Config::production(LogSink, Wake)`.
  - `controller::{EntryLoad, MetadataLoad, LogSink, Wake}` (tipos `Arc<dyn Fn ...>`). `Wake` é chamado por qualquer thread do controlador ao entregar uma resposta.
  - `controller::Model { display, clock, queue: QueueState, entries: HashMap<String, QueueEntryInfo>, volume: VolumeState, error: Option<String> }`.
  - `controller::QueueState { items: Vec<Track>, current_index: Option<u32>, is_playing: bool }`, com `current()` e `current_item_id()`.
  - `controller::VolumeState { value: f64, muted: bool }`.
  - `controller::Changes { display, clock, queue, entries, volume, error: bool }`, com `Changes::NONE` e `any()`.
  - Crate `hog-player-testing`: `FakePlayer` (`new()`, `with(..)`, `acrescentar(&str)`, `chamou(&str)`, `esperar(Duration, ..)`), `FakeState`, `Item`, `itens(&[&str])`, `Gate`, `PRAZO`, `controlador(&Arc<FakePlayer>)`, `config_de_teste()`, `entrada(&str)`, `metadados(&str)`, `sem_log()`, `pasta_com(&str, &[&str])`.

- [ ] **Step 1: O crate de teste com o `FakePlayer`**

`apps/gpui/Cargo.toml`: a lista de membros passa a ser `members = ["model", "app", "testing"]`.

`apps/gpui/testing/Cargo.toml`:

```toml
[package]
name = "hog-player-testing"
version.workspace = true
edition.workspace = true
publish.workspace = true

[dependencies]
hog-audio.workspace = true
hog-player-model = { path = "../model" }
```

`apps/gpui/model/Cargo.toml`: acrescentar

```toml
[dev-dependencies]
hog-player-testing = { path = "../testing" }
```

`apps/gpui/testing/src/lib.rs`:

```rust
//! O dublê do `HogPlayer` e os helpers dos testes: porte do `FakePlayer` de
//! `ViewModelTests.swift`. Crate só de teste, para o `src/` dos outros crates ficar só com código
//! de produção. Registra o que foi chamado e deixa o teste segurar comandos com portões.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hog_audio::api::{PlayerError, QueueItem, Snapshot, TrackFormat, TransitionTimings};
use hog_audio::transitions::PlayerState;
use hog_player_model::controller::{Config, Controller, LogSink};
use hog_player_model::entry::QueueEntryInfo;
use hog_player_model::metadata::TrackMetadata;
use hog_player_model::port::Player;

pub const PRAZO: Duration = Duration::from_secs(2);

/// Segura uma chamada até o teste abrir: prova ordem e descarte sem depender de relógio.
#[derive(Default)]
pub struct Gate {
    open: Mutex<bool>,
    wake: Condvar,
}

impl Gate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn wait(&self) {
        let mut open = self.open.lock().unwrap_or_else(|p| p.into_inner());
        while !*open {
            open = self.wake.wait(open).unwrap_or_else(|p| p.into_inner());
        }
    }

    pub fn open(&self) {
        *self.open.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.wake.notify_all();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u64,
    pub path: String,
    pub failure: Option<String>,
}

/// Ids sequenciais a partir de 1, como o engine dá a uma fila nova.
pub fn itens(paths: &[&str]) -> Vec<Item> {
    paths
        .iter()
        .enumerate()
        .map(|(i, path)| Item { id: i as u64 + 1, path: path.to_string(), failure: None })
        .collect()
}

pub struct FakeState {
    pub chamadas: Vec<String>,
    pub estado: PlayerState,
    pub volume_aplicado: Option<f32>,
    /// O volume que o device "de verdade" tem: é o que o snapshot devolve.
    pub volume_real: Option<f32>,
    pub mudo: bool,
    pub falha_ao_aplicar_volume: Option<String>,
    /// O atraso vale só para o escalar indicado, para identificar a escrita certa.
    pub atraso_de_volume: Duration,
    pub escalar_com_atraso: Option<f32>,
    pub escritas_de_volume: usize,
    pub itens: Vec<Item>,
    pub indice_atual: Option<u32>,
    pub versao_fila: u64,
    pub leituras_da_fila: usize,
    pub avancos: usize,
    pub atraso_de_avanco: Duration,
    pub atraso_de_selecao: Duration,
    pub portao_da_selecao: Option<Arc<Gate>>,
    pub selecao_esperando: bool,
    pub decorrido: f64,
    pub suspeita_de_device: bool,
    pub seq_problema: u64,
    pub problema: Option<String>,
    pub recuperacoes: usize,
    /// O aviso que o engine publica no fim de um Play, e o portão que segura o `play()` depois
    /// de publicá-lo: reproduz um tique exibindo o aviso antes de o comando voltar.
    pub aviso_ao_tocar: Option<String>,
    pub portao_do_play: Option<Arc<Gate>>,
    /// O engine ocupado numa troca de device: `try_queue_items` desiste em vez de esperar.
    pub fila_ocupada: bool,
    pub navegacoes: Vec<i32>,
}

impl Default for FakeState {
    fn default() -> Self {
        Self {
            chamadas: Vec::new(),
            estado: PlayerState::Idle,
            volume_aplicado: None,
            volume_real: Some(0.5),
            mudo: false,
            falha_ao_aplicar_volume: None,
            atraso_de_volume: Duration::ZERO,
            escalar_com_atraso: None,
            escritas_de_volume: 0,
            itens: Vec::new(),
            indice_atual: None,
            versao_fila: 0,
            leituras_da_fila: 0,
            avancos: 0,
            atraso_de_avanco: Duration::ZERO,
            atraso_de_selecao: Duration::ZERO,
            portao_da_selecao: None,
            selecao_esperando: false,
            decorrido: 0.0,
            suspeita_de_device: false,
            seq_problema: 0,
            problema: None,
            recuperacoes: 0,
            aviso_ao_tocar: None,
            portao_do_play: None,
            fila_ocupada: false,
            navegacoes: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct FakePlayer {
    state: Mutex<FakeState>,
}

impl FakePlayer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut FakeState) -> R) -> R {
        f(&mut self.lock())
    }

    /// Acrescenta sem mexer na versão da fila: o teste decide quando o controlador relê.
    pub fn acrescentar(&self, path: &str) {
        let mut s = self.lock();
        let item = novo_item(&s.itens, path);
        s.itens.push(item);
    }

    pub fn chamou(&self, nome: &str) -> bool {
        self.lock().chamadas.iter().any(|c| c == nome)
    }

    /// Espera o dublê chegar a um estado, com teto; devolve se chegou.
    pub fn esperar(&self, prazo: Duration, condicao: impl Fn(&FakeState) -> bool) -> bool {
        let limite = Instant::now() + prazo;
        while Instant::now() < limite {
            if condicao(&self.lock()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }
}

fn novo_item(itens: &[Item], path: &str) -> Item {
    let id = itens.iter().map(|i| i.id).max().unwrap_or(0) + 1;
    Item { id, path: path.to_string(), failure: None }
}

fn track() -> TrackFormat {
    TrackFormat {
        sample_rate: 44_100.0,
        bit_depth: 16,
        channels: 2,
        codec: "flac".to_string(),
        device_name: "Fake".to_string(),
        total_seconds: 10.0,
    }
}

fn state_error(message: &str) -> PlayerError {
    PlayerError::State { message: message.to_string() }
}

fn queue_items(itens: &[Item]) -> Vec<QueueItem> {
    itens
        .iter()
        .map(|i| QueueItem { id: i.id, path: i.path.clone(), failure: i.failure.clone() })
        .collect()
}

impl Player for FakePlayer {
    fn append_tracks(&self, paths: Vec<String>) -> u32 {
        let mut s = self.lock();
        for path in &paths {
            let item = novo_item(&s.itens, path);
            s.itens.push(item);
        }
        if s.indice_atual.is_none() && !s.itens.is_empty() {
            s.indice_atual = Some(0);
            s.estado = PlayerState::Loaded;
        }
        if !paths.is_empty() {
            s.versao_fila += 1;
        }
        paths.len() as u32
    }

    fn queue_items(&self) -> Vec<QueueItem> {
        let mut s = self.lock();
        s.leituras_da_fila += 1;
        queue_items(&s.itens)
    }

    fn try_queue_items(&self) -> Option<Vec<QueueItem>> {
        let mut s = self.lock();
        if s.fila_ocupada {
            return None;
        }
        s.leituras_da_fila += 1;
        Some(queue_items(&s.itens))
    }

    fn navigate(&self, steps: i32) -> Result<TrackFormat, PlayerError> {
        let mut s = self.lock();
        s.navegacoes.push(steps);
        let Some(current) = s.indice_atual.filter(|_| !s.itens.is_empty()) else {
            return Err(state_error("sem faixa"));
        };
        let alvo = (i64::from(current) + i64::from(steps)).clamp(0, s.itens.len() as i64 - 1);
        s.indice_atual = Some(alvo as u32);
        if s.estado != PlayerState::Playing {
            s.estado = PlayerState::Loaded;
        }
        Ok(track())
    }

    fn select_track(&self, index: u32) -> Result<TrackFormat, PlayerError> {
        let (atraso, portao) = {
            let mut s = self.lock();
            s.selecao_esperando = true;
            (s.atraso_de_selecao, s.portao_da_selecao.clone())
        };
        std::thread::sleep(atraso);
        if let Some(portao) = portao {
            portao.wait();
        }
        let mut s = self.lock();
        if index as usize >= s.itens.len() {
            return Err(state_error("índice inválido"));
        }
        s.chamadas.push("select".to_string());
        s.indice_atual = Some(index);
        if s.estado != PlayerState::Playing {
            s.estado = PlayerState::Loaded;
        }
        Ok(track())
    }

    fn select_track_id(&self, id: u64) -> Result<TrackFormat, PlayerError> {
        let index = self.lock().itens.iter().position(|i| i.id == id);
        match index {
            Some(index) => self.select_track(index as u32),
            None => Err(state_error("a faixa não está mais na fila")),
        }
    }

    fn advance(&self) -> Result<Option<TrackFormat>, PlayerError> {
        let atraso = self.lock().atraso_de_avanco;
        std::thread::sleep(atraso);
        let mut s = self.lock();
        s.avancos += 1;
        let Some(current) = s.indice_atual else { return Ok(None) };
        if current as usize + 1 >= s.itens.len() {
            return Ok(None);
        }
        s.indice_atual = Some(current + 1);
        s.estado = PlayerState::Playing;
        Ok(Some(track()))
    }

    /// Como o engine: removida a atual, quem assume a posição é carregado aqui mesmo e o
    /// formato dela volta na resposta.
    fn remove_track_id(&self, id: u64) -> Result<Option<TrackFormat>, PlayerError> {
        let mut s = self.lock();
        let Some(index) = s.itens.iter().position(|i| i.id == id) else {
            return Err(state_error("a faixa não está mais na fila"));
        };
        s.chamadas.push("remove".to_string());
        let before = s.indice_atual.map(|i| i as usize);
        s.itens.remove(index);
        s.versao_fila += 1;
        if before == Some(index) {
            if index >= s.itens.len() {
                s.indice_atual = None;
                s.estado = PlayerState::Idle;
                return Ok(None);
            }
            s.indice_atual = Some(index as u32);
            return Ok(Some(track()));
        }
        if let Some(before) = before
            && index < before
        {
            s.indice_atual = Some(before as u32 - 1);
        }
        Ok(None)
    }

    fn clear_queue(&self) -> Result<(), PlayerError> {
        let mut s = self.lock();
        s.itens.clear();
        s.indice_atual = None;
        s.estado = PlayerState::Idle;
        s.versao_fila += 1;
        Ok(())
    }

    fn play(&self) -> Result<(), PlayerError> {
        let portao = {
            let mut s = self.lock();
            s.chamadas.push("play".to_string());
            s.estado = PlayerState::Playing;
            if let Some(aviso) = s.aviso_ao_tocar.clone() {
                s.problema = Some(aviso);
                s.seq_problema += 1;
            }
            s.portao_do_play.clone()
        };
        if let Some(portao) = portao {
            portao.wait();
        }
        Ok(())
    }

    fn pause(&self) -> Result<(), PlayerError> {
        let mut s = self.lock();
        s.chamadas.push("pause".to_string());
        s.estado = PlayerState::Paused;
        Ok(())
    }

    fn set_volume(&self, scalar: f32) -> Result<(), PlayerError> {
        let (atraso, alvo) = {
            let s = self.lock();
            (s.atraso_de_volume, s.escalar_com_atraso)
        };
        if alvo == Some(scalar) {
            std::thread::sleep(atraso);
        }
        let mut s = self.lock();
        s.escritas_de_volume += 1;
        if let Some(message) = s.falha_ao_aplicar_volume.clone() {
            return Err(PlayerError::Device { message });
        }
        s.volume_aplicado = Some(scalar);
        s.volume_real = Some(scalar);
        Ok(())
    }

    fn snapshot(&self) -> Snapshot {
        let s = self.lock();
        Snapshot {
            state: s.estado,
            elapsed_seconds: s.decorrido,
            total_seconds: 10.0,
            underruns: 0,
            volume_scalar: s.volume_real,
            muted: s.mudo,
            current_index: s.indice_atual,
            queue_len: s.itens.len() as u32,
            queue_version: s.versao_fila,
            device_suspect: s.suspeita_de_device,
            problem_seq: s.seq_problema,
        }
    }

    fn recover(&self) -> Result<bool, PlayerError> {
        let mut s = self.lock();
        s.chamadas.push("recover".to_string());
        s.recuperacoes += 1;
        if !s.suspeita_de_device {
            return Ok(false);
        }
        s.suspeita_de_device = false;
        s.estado = PlayerState::Loaded;
        s.problema = Some("o device de saída foi desconectado; a reprodução parou".to_string());
        s.seq_problema += 1;
        Ok(true)
    }

    fn last_problem(&self) -> Option<String> {
        self.lock().problema.clone()
    }

    fn last_timings(&self) -> TransitionTimings {
        TransitionTimings {
            open_ms: 0.0,
            release_ms: 0.0,
            reopen_ms: 0.0,
            acquire_ms: 0.0,
            volume_ms: 0.0,
            prefill_ms: 0.0,
            start_ms: 0.0,
        }
    }

    fn shutdown(&self) -> Result<(), PlayerError> {
        self.lock().chamadas.push("shutdown".to_string());
        Ok(())
    }
}

pub fn entrada(title: &str) -> QueueEntryInfo {
    QueueEntryInfo {
        title: title.to_string(),
        artist: None,
        album: None,
        quality: None,
        quality_detail: None,
        duration: None,
        thumbnail: None,
        problem: None,
    }
}

pub fn metadados(title: &str) -> TrackMetadata {
    TrackMetadata { title: title.to_string(), artist: None, album: None, artwork: None }
}

pub fn sem_log() -> LogSink {
    Arc::new(|_: &str| {})
}

/// Cargas instantâneas: a linha vira "fila <caminho>" e os metadados, o próprio caminho. Nos
/// testes quem chama `tick()` é o próprio teste, então acordar não faz nada.
pub fn config_de_teste() -> Config {
    Config {
        load_entry: Arc::new(|path: &str| entrada(&format!("fila {path}"))),
        load_metadata: Arc::new(|path: &str| metadados(path)),
        log: sem_log(),
        wake: Arc::new(|| {}),
    }
}

pub fn controlador(fake: &Arc<FakePlayer>) -> Controller {
    Controller::new(fake.clone(), config_de_teste())
}

/// Uma pasta temporária nova, com arquivos vazios dos nomes pedidos.
pub fn pasta_com(nome: &str, arquivos: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("hog-gpui-{nome}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("pasta temporária");
    for arquivo in arquivos {
        std::fs::write(root.join(arquivo), b"").expect("arquivo temporário");
    }
    root
}
```

- [ ] **Step 2: Testes de transporte (vermelho)**

`apps/gpui/model/tests/controller_transport.rs`:

```rust
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hog_audio::transitions::PlayerState;
use hog_player_model::controller::{Config, Controller, MetadataLoad};
use hog_player_model::display::NO_TRACK_TITLE;
use std::sync::atomic::{AtomicUsize, Ordering};

use hog_player_testing::{
    FakePlayer, Gate, PRAZO, config_de_teste, controlador, entrada, itens, metadados, pasta_com,
};

fn carregada(paths: &[&str], estado: PlayerState) -> Arc<FakePlayer> {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(paths);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = estado;
    });
    fake
}

fn tocou(fake: &FakePlayer) -> bool {
    fake.esperar(PRAZO, |s| s.chamadas.iter().any(|c| c == "play"))
}

#[test]
fn o_botao_toca_quando_parado_e_pausa_quando_tocando() {
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);

    fake.with(|s| s.estado = PlayerState::Loaded);
    c.tick();
    c.toggle();
    assert!(c.wait_idle(PRAZO));
    assert!(fake.chamou("play"));

    fake.with(|s| s.estado = PlayerState::Playing);
    c.tick();
    c.toggle();
    assert!(c.wait_idle(PRAZO));
    assert!(fake.chamou("pause"));
}

#[test]
fn sem_faixa_carregada_o_botao_nao_faz_nada() {
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);
    c.tick();
    c.toggle();
    assert!(c.wait_idle(PRAZO));
    assert!(fake.with(|s| s.chamadas.is_empty()));
}

#[test]
fn o_tick_atualiza_o_que_a_tela_mostra() {
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);
    fake.with(|s| s.estado = PlayerState::Playing);
    c.tick();
    assert!(c.model().display.is_playing);
    assert!(c.model().display.can_pause);
}

/// Com outro comando na fila atrás do toggle, o acompanhamento do volume externo está travado:
/// só a releitura do fim do toggle pode trazer o volume real.
#[test]
fn o_toggle_rele_o_volume_real_do_device_ao_fim_do_comando() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Loaded);
    let portao = Gate::new();
    fake.with(|s| {
        s.volume_real = Some(0.82);
        s.portao_da_selecao = Some(portao.clone());
    });
    let mut c = controlador(&fake);
    c.tick();

    c.toggle();
    c.select(2);
    assert!(fake.esperar(PRAZO, |s| s.selecao_esperando));
    c.tick();

    assert_eq!(c.model().volume.value, f64::from(0.82_f32));
    portao.open();
    assert!(c.wait_idle(PRAZO));
}

#[test]
fn finished_dispara_um_unico_avanco_enquanto_a_carga_corre() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Finished);
    fake.with(|s| s.atraso_de_avanco = Duration::from_millis(150));
    let mut c = controlador(&fake);

    for _ in 0..6 {
        c.tick();
    }
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.avancos), 1);
    assert_eq!(c.model().queue.current_index, Some(1));
}

#[test]
fn next_manual_pendente_impede_auto_avanco_do_mesmo_finished() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"], PlayerState::Finished);
    let mut c = controlador(&fake);

    c.next();
    c.tick();
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.avancos), 0);
    assert_eq!(c.model().queue.current_index, Some(1));
}

#[test]
fn selecao_e_play_preservam_a_ordem_de_chegada() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Loaded);
    fake.with(|s| s.atraso_de_selecao = Duration::from_millis(100));
    let mut c = controlador(&fake);

    c.select(2);
    c.toggle();
    assert!(c.wait_idle(PRAZO));

    let chamadas = fake.with(|s| s.chamadas.clone());
    assert_eq!(chamadas[chamadas.len() - 2..], ["select", "play"]);
    assert_eq!(fake.with(|s| (s.indice_atual, s.estado)), (Some(1), PlayerState::Playing));
}

/// O play fica preso no portão segurando a thread de comandos; a seleção espera a vez atrás
/// dele. O shutdown chega nesse meio: o que ainda não começou é descartado.
#[test]
fn shutdown_descarta_o_que_ainda_nao_chegou_ao_player() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Loaded);
    let portao = Gate::new();
    fake.with(|s| s.portao_do_play = Some(portao.clone()));
    let mut c = controlador(&fake);

    c.toggle();
    assert!(tocou(&fake));
    c.select(2);
    c.shutdown().expect("shutdown");
    portao.open();
    assert!(c.wait_idle(PRAZO));

    assert!(!fake.chamou("select"));
    assert!(fake.chamou("shutdown"));
    assert_eq!(fake.with(|s| s.indice_atual), Some(0));
}

#[test]
fn remocao_classifica_a_faixa_atual_depois_da_selecao_pendente() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Loaded);
    fake.with(|s| s.atraso_de_selecao = Duration::from_millis(100));
    let mut c = controlador(&fake);

    c.select(2);
    c.remove(2);
    assert!(c.wait_idle(PRAZO));

    assert_eq!(c.model().queue.current_index, None);
    assert_eq!(c.model().display.title, NO_TRACK_TITLE);
    let paths: Vec<_> = c.model().queue.items.iter().map(|t| t.path.as_str()).collect();
    assert_eq!(paths, ["/tmp/a.flac"]);
}

#[test]
fn duas_remocoes_seguidas_apagam_as_faixas_certas() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"], PlayerState::Loaded);
    let mut c = controlador(&fake);

    c.remove(2);
    c.remove(3);
    assert!(c.wait_idle(PRAZO));

    let paths: Vec<_> = fake.with(|s| s.itens.iter().map(|i| i.path.clone()).collect());
    assert_eq!(paths, ["/tmp/a.flac"]);
    assert_eq!(c.model().error, None);
}

#[test]
fn remover_a_faixa_atual_nao_carrega_de_novo_quem_assumiu() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"], PlayerState::Playing);
    let mut c = controlador(&fake);

    c.remove(1);
    assert!(c.wait_idle(PRAZO));

    assert!(!fake.chamou("select"));
    assert_eq!(c.model().queue.current_index, Some(0));
    assert_eq!(c.model().queue.current_item_id(), Some(2));
    assert!(c.model().display.technical_line.contains("44.1 kHz"));
}

#[test]
fn problema_do_device_aparece_uma_vez_so() {
    let fake = carregada(&["/tmp/a.flac"], PlayerState::Playing);
    let mut c = controlador(&fake);

    fake.with(|s| {
        s.problema = Some("o device sumiu".to_string());
        s.seq_problema = 1;
    });
    c.tick();
    assert_eq!(c.model().error.as_deref(), Some("o device sumiu"));

    c.apply_volume(0.3);
    assert!(c.wait_idle(PRAZO));
    assert_eq!(c.model().error, None);
    c.tick();
    assert_eq!(c.model().error, None);
}

#[test]
fn aviso_publicado_no_play_nao_some_quando_o_play_termina() {
    let fake = carregada(&["/tmp/a.flac"], PlayerState::Loaded);
    let portao = Gate::new();
    fake.with(|s| {
        s.aviso_ao_tocar = Some("o volume voltou ao teto".to_string());
        s.portao_do_play = Some(portao.clone());
    });
    let mut c = controlador(&fake);
    c.tick();

    c.toggle();
    assert!(fake.esperar(PRAZO, |s| s.seq_problema > 0));
    c.tick();
    assert_eq!(c.model().error.as_deref(), Some("o volume voltou ao teto"));

    portao.open();
    assert!(c.wait_idle(PRAZO));
    assert_eq!(c.model().error.as_deref(), Some("o volume voltou ao teto"));
}

#[test]
fn problema_de_antes_de_abrir_nao_aparece() {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.problema = Some("de uma sessão anterior".to_string());
        s.seq_problema = 3;
    });
    let mut c = controlador(&fake);
    c.tick();
    assert_eq!(c.model().error, None);
}

#[test]
fn device_suspeito_no_fim_da_faixa_recupera_e_nao_avanca() {
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Finished);
    fake.with(|s| s.suspeita_de_device = true);
    let mut c = controlador(&fake);

    c.tick();
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| (s.recuperacoes, s.avancos, s.estado)), (1, 0, PlayerState::Loaded));
    assert!(c.model().error.as_deref().is_some_and(|e| e.contains("desconectado")));
}

#[test]
fn suspeita_repetida_no_tick_nao_enfileira_outra_recuperacao() {
    let fake = carregada(&["/tmp/a.flac"], PlayerState::Playing);
    fake.with(|s| s.suspeita_de_device = true);
    let mut c = controlador(&fake);

    c.tick();
    c.tick();
    c.tick();
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.recuperacoes), 1);
}

#[test]
fn dois_toggles_rapidos_tocam_e_depois_pausam() {
    let fake = carregada(&["/tmp/a.flac"], PlayerState::Loaded);
    let mut c = controlador(&fake);

    c.toggle();
    c.toggle();
    assert!(c.wait_idle(PRAZO));

    let chamadas = fake.with(|s| s.chamadas.clone());
    assert_eq!(chamadas[chamadas.len() - 2..], ["play", "pause"]);
    assert_eq!(fake.with(|s| s.estado), PlayerState::Paused);
}

/// Cinco faixas carregadas, com os metadados vindos do `load` dado.
fn fila_com_metadados(load: MetadataLoad) -> (Controller, Arc<FakePlayer>) {
    let fake = carregada(
        &["/tmp/0.flac", "/tmp/1.flac", "/tmp/2.flac", "/tmp/3.flac", "/tmp/4.flac"],
        PlayerState::Loaded,
    );
    let config = Config {
        load_entry: Arc::new(|path: &str| hog_player_model::entry::QueueEntryInfo {
            artist: Some("Artista".to_string()),
            album: Some("Álbum".to_string()),
            ..entrada(&format!("fila {path}"))
        }),
        load_metadata: load,
        ..config_de_teste()
    };
    (Controller::new(fake.clone(), config), fake)
}

#[test]
fn next_nao_espera_os_metadados() {
    let portao = Gate::new();
    let segura = portao.clone();
    let (mut c, fake) = fila_com_metadados(Arc::new(move |path: &str| {
        segura.wait();
        metadados(&format!("meta {path}"))
    }));

    c.next();
    assert!(c.wait_idle(PRAZO), "o comando ficou esperando a leitura de metadados");
    assert_eq!(fake.with(|s| s.navegacoes.clone()), [1]);
    assert_eq!(c.model().queue.current_index, Some(1));

    portao.open();
    assert!(c.wait_metadata(PRAZO));
    assert_eq!(c.model().display.title, "meta /tmp/1.flac");
}

#[test]
fn o_titulo_vem_da_fila_antes_dos_metadados() {
    let portao = Gate::new();
    let segura = portao.clone();
    let (mut c, _) = fila_com_metadados(Arc::new(move |path: &str| {
        segura.wait();
        metadados(&format!("meta {path}"))
    }));
    assert!(c.wait_entries(PRAZO));

    c.next();
    assert!(c.wait_idle(PRAZO));
    assert_eq!(c.model().display.title, "fila /tmp/1.flac");
    assert_eq!(c.model().display.artist, "Artista");

    portao.open();
    assert!(c.wait_metadata(PRAZO));
}

#[test]
fn metadado_atrasado_de_faixa_antiga_e_descartado() {
    let portao = Gate::new();
    let segura = portao.clone();
    let (mut c, _) = fila_com_metadados(Arc::new(move |path: &str| {
        if path == "/tmp/1.flac" {
            segura.wait();
        }
        metadados(&format!("meta {path}"))
    }));

    c.next();
    assert!(c.wait_idle(PRAZO));
    c.next();
    assert!(c.wait_idle(PRAZO));
    assert!(c.wait_until(PRAZO, |c| c.model().display.title == "meta /tmp/2.flac"));

    portao.open();
    assert!(c.wait_metadata(PRAZO));
    assert_eq!(c.model().display.title, "meta /tmp/2.flac");
}

#[test]
fn tres_nexts_rapidos_viram_uma_carga_so() {
    let (mut c, fake) = fila_com_metadados(Arc::new(|path: &str| metadados(path)));
    let portao = Gate::new();
    fake.with(|s| s.portao_do_play = Some(portao.clone()));

    c.toggle();
    assert!(tocou(&fake));
    c.next();
    c.next();
    c.next();
    portao.open();
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.navegacoes.clone()), [3]);
    assert_eq!(c.model().queue.current_index, Some(3));
}

#[test]
fn next_e_previous_seguidos_nao_carregam_nada() {
    let (mut c, fake) = fila_com_metadados(Arc::new(|path: &str| metadados(path)));
    let portao = Gate::new();
    fake.with(|s| s.portao_do_play = Some(portao.clone()));

    c.toggle();
    assert!(tocou(&fake));
    c.next();
    c.previous();
    portao.open();
    assert!(c.wait_idle(PRAZO));

    assert!(fake.with(|s| s.navegacoes.is_empty()));
    assert_eq!(c.model().queue.current_index, Some(0));
}

#[test]
fn soltar_audio_numa_fila_vazia_seleciona_a_primeira() {
    let pasta = pasta_com("soltar-vazia", &["b.flac", "a.flac", "capa.jpg"]);
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);

    c.add(vec![pasta.clone()]);
    assert!(c.wait_idle(PRAZO));

    let paths: Vec<String> = fake.with(|s| s.itens.iter().map(|i| i.path.clone()).collect());
    assert_eq!(paths.len(), 2);
    assert!(paths[0].ends_with("/a.flac") && paths[1].ends_with("/b.flac"), "{paths:?}");
    assert!(fake.chamou("select"));
    assert_eq!(c.model().error, None);
    std::fs::remove_dir_all(pasta).unwrap();
}

#[test]
fn soltar_numa_fila_com_faixas_nao_seleciona() {
    let pasta = pasta_com("soltar-cheia", &["c.flac"]);
    let fake = carregada(&["/tmp/a.flac"], PlayerState::Loaded);
    let mut c = controlador(&fake);

    c.add(vec![pasta.clone()]);
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.itens.len()), 2);
    assert!(!fake.chamou("select"));
    std::fs::remove_dir_all(pasta).unwrap();
}

#[test]
fn soltar_so_o_que_nao_e_audio_mostra_erro() {
    let pasta = pasta_com("soltar-sem-audio", &["capa.jpg"]);
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);

    c.add(vec![pasta.clone()]);
    assert!(c.wait_idle(PRAZO));

    assert_eq!(c.model().error.as_deref(), Some("nenhum arquivo de áudio encontrado"));
    assert!(fake.with(|s| s.itens.is_empty()));
    std::fs::remove_dir_all(pasta).unwrap();
}

#[test]
fn cada_transicao_gera_uma_linha_no_formato_do_swift() {
    let linhas = Arc::new(Mutex::new(Vec::<String>::new()));
    let guarda = linhas.clone();
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Loaded);
    let config = Config {
        log: Arc::new(move |linha: &str| guarda.lock().unwrap().push(linha.to_string())),
        ..config_de_teste()
    };
    let mut c = Controller::new(fake.clone(), config);

    let antes = Instant::now();
    c.select(2);
    assert!(c.wait_idle(PRAZO));
    assert!(antes.elapsed() < PRAZO);

    let linhas = linhas.lock().unwrap();
    assert_eq!(linhas.len(), 1, "{linhas:?}");
    assert!(linhas[0].starts_with("seleção: espera "), "{}", linhas[0]);
    assert!(
        linhas[0].contains("(open 0.0, release 0.0, reopen 0.0, acquire 0.0, volume 0.0, prefill 0.0, start 0.0)"),
        "{}",
        linhas[0]
    );
}

/// No Swift o fim de um comando atualiza a tela na hora (o `apply` chama `refresh()`). Aqui a
/// thread acorda quem desenha ao entregar a resposta, sem esperar o próximo tique do poll.
#[test]
fn cada_resposta_acorda_quem_desenha() {
    let acordou = Arc::new(AtomicUsize::new(0));
    let conta = acordou.clone();
    let fake = carregada(&["/tmp/a.flac", "/tmp/b.flac"], PlayerState::Loaded);
    let config = Config {
        wake: Arc::new(move || {
            conta.fetch_add(1, Ordering::SeqCst);
        }),
        ..config_de_teste()
    };
    let mut c = Controller::new(fake.clone(), config);
    let antes = acordou.load(Ordering::SeqCst);

    c.select(2);

    let limite = Instant::now() + PRAZO;
    while acordou.load(Ordering::SeqCst) == antes && Instant::now() < limite {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(acordou.load(Ordering::SeqCst) > antes, "a resposta do comando não acordou ninguém");
    assert!(c.wait_idle(PRAZO));
}
```

- [ ] **Step 3: Testes de volume (vermelho)**

`apps/gpui/model/tests/controller_volume.rs`:

```rust
use std::sync::Arc;
use std::time::Duration;

use hog_audio::transitions::PlayerState;
use hog_player_model::controller::Controller;
use hog_player_testing::{FakePlayer, PRAZO, controlador, itens};

fn tocando_com_volume(volume: Option<f32>) -> (Controller, Arc<FakePlayer>) {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&["/tmp/a.flac", "/tmp/b.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Playing;
        s.volume_real = volume;
    });
    let mut c = controlador(&fake);
    c.tick();
    (c, fake)
}

#[test]
fn o_volume_vai_de_zero_a_um_para_o_engine() {
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);
    c.apply_volume(0.35);
    assert!(c.wait_idle(PRAZO));
    assert_eq!(fake.with(|s| s.volume_aplicado), Some(0.35));
}

#[test]
fn volume_que_falha_volta_ao_valor_do_device_e_nao_ao_pedido() {
    let fake = FakePlayer::new();
    fake.with(|s| s.falha_ao_aplicar_volume = Some("device recusou o volume".to_string()));
    let mut c = controlador(&fake);

    c.apply_volume(0.9);
    assert!(c.wait_idle(PRAZO));

    assert!(c.model().error.is_some());
    assert_eq!(c.model().volume.value, 0.5);
}

#[test]
fn chamadas_consecutivas_de_volume_aplicam_a_ultima_pedida() {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.escalar_com_atraso = Some(0.25);
        s.atraso_de_volume = Duration::from_millis(150);
    });
    let mut c = controlador(&fake);

    c.apply_volume(0.25);
    c.apply_volume(0.75);
    assert!(c.wait_idle(PRAZO));

    assert_eq!(fake.with(|s| s.volume_aplicado), Some(0.75));
}

#[test]
fn o_arrasto_do_slider_nao_vira_uma_escrita_por_quadro() {
    let fake = FakePlayer::new();
    let mut c = controlador(&fake);

    for i in 0..50 {
        c.apply_volume(f64::from(i) / 50.0);
    }
    assert!(c.wait_idle(PRAZO));

    assert!(fake.with(|s| s.escritas_de_volume) < 50);
    assert_eq!(fake.with(|s| s.volume_aplicado), Some((49.0 / 50.0) as f32));
    assert_eq!(c.model().volume.value, 49.0 / 50.0);
}

#[test]
fn erro_de_volume_some_depois_de_um_comando_bem_sucedido() {
    let fake = FakePlayer::new();
    fake.with(|s| s.falha_ao_aplicar_volume = Some("device recusou o volume".to_string()));
    let mut c = controlador(&fake);

    c.apply_volume(0.9);
    assert!(c.wait_idle(PRAZO));
    assert!(c.model().error.is_some());

    fake.with(|s| s.falha_ao_aplicar_volume = None);
    c.apply_volume(0.4);
    assert!(c.wait_idle(PRAZO));
    assert_eq!(c.model().error, None);
}

#[test]
fn o_slider_segue_o_volume_mudado_por_fora() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));
    fake.with(|s| s.volume_real = Some(0.25));
    c.tick();
    assert_eq!(c.model().volume.value, 0.25);
}

#[test]
fn o_slider_nao_segue_o_volume_fora_de_tocar_ou_pausar() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));
    fake.with(|s| {
        s.estado = PlayerState::Loaded;
        s.volume_real = Some(0.25);
    });
    c.tick();
    assert_eq!(c.model().volume.value, 0.5);
}

#[test]
fn o_slider_nao_pisca_na_troca_de_faixa() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));
    fake.with(|s| s.atraso_de_selecao = Duration::from_millis(100));

    c.select(2);
    fake.with(|s| s.volume_real = Some(0.9));
    c.tick();
    assert_eq!(c.model().volume.value, 0.5);

    fake.with(|s| s.volume_real = Some(0.5));
    assert!(c.wait_idle(PRAZO));
}

#[test]
fn o_slider_nao_sobrescreve_um_arrasto() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));

    c.set_volume_editing(true);
    fake.with(|s| s.volume_real = Some(0.25));
    c.tick();
    assert_eq!(c.model().volume.value, 0.5);

    c.set_volume_editing(false);
    c.tick();
    assert_eq!(c.model().volume.value, 0.25);
}

#[test]
fn o_slider_nao_volta_enquanto_a_escrita_nao_chegou_ao_device() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));
    fake.with(|s| {
        s.escalar_com_atraso = Some(0.25);
        s.atraso_de_volume = Duration::from_millis(150);
    });

    c.apply_volume(0.25);
    c.tick();
    assert_eq!(c.model().volume.value, 0.25);

    assert!(c.wait_idle(PRAZO));
}

#[test]
fn device_sem_controle_de_volume_nao_mexe_no_slider() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));
    fake.with(|s| s.volume_real = None);
    c.tick();
    assert_eq!(c.model().volume.value, 0.5);
}

/// 0.35 em `f32` e de volta em `f64` vira 0.3499999…: o slider mostraria 34%.
#[test]
fn volume_igual_ao_do_device_nao_arredonda_o_slider() {
    let (mut c, _) = tocando_com_volume(Some(0.5));
    c.apply_volume(0.35);
    assert!(c.wait_idle(PRAZO));
    c.tick();
    assert_eq!(c.model().volume.value, 0.35);
}

#[test]
fn o_mudo_do_device_aparece_na_tela() {
    let (mut c, fake) = tocando_com_volume(Some(0.5));
    fake.with(|s| s.mudo = true);
    c.tick();
    assert!(c.model().volume.muted);
}
```

- [ ] **Step 4: Testes da fila e das linhas (vermelho)**

`apps/gpui/model/tests/controller_queue.rs`:

```rust
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hog_audio::transitions::PlayerState;
use hog_player_model::controller::{Config, Controller, EntryLoad};
use hog_player_model::entry::QueueEntryInfo;
use hog_player_testing::{FakePlayer, PRAZO, config_de_teste, controlador, entrada, itens};

#[test]
fn a_fila_so_e_relida_quando_a_versao_muda() {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&["/tmp/a.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Loaded;
    });
    let mut c = controlador(&fake);

    c.tick();
    c.tick();
    assert_eq!(fake.with(|s| s.leituras_da_fila), 1);

    fake.acrescentar("/tmp/b.flac");
    fake.with(|s| s.versao_fila = 2);
    c.tick();
    assert_eq!(fake.with(|s| s.leituras_da_fila), 2);
    assert_eq!(c.model().queue.items.len(), 2);
}

#[test]
fn fila_ocupada_nao_bloqueia_e_rele_depois() {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&["/tmp/0.flac", "/tmp/1.flac", "/tmp/2.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Loaded;
    });
    let mut c = controlador(&fake);
    fake.acrescentar("/tmp/nova.flac");
    fake.with(|s| {
        s.versao_fila += 1;
        s.fila_ocupada = true;
    });

    c.tick();
    assert_eq!(c.model().queue.items.len(), 3);

    fake.with(|s| s.fila_ocupada = false);
    c.tick();
    assert_eq!(c.model().queue.items.len(), 4);
}

#[derive(Default)]
struct Registro {
    em_andamento: usize,
    pico: usize,
    ordem: Vec<String>,
}

/// O atraso só mantém as cargas em voo ao mesmo tempo; o teste espera a carga, nunca o relógio.
fn carregador(registro: Arc<Mutex<Registro>>) -> EntryLoad {
    Arc::new(move |path: &str| {
        {
            let mut r = registro.lock().unwrap();
            r.em_andamento += 1;
            r.pico = r.pico.max(r.em_andamento);
            r.ordem.push(path.to_string());
        }
        std::thread::sleep(Duration::from_millis(20));
        registro.lock().unwrap().em_andamento -= 1;
        QueueEntryInfo {
            quality: Some("24/96".to_string()),
            duration: Some("1:00".to_string()),
            ..entrada(&format!("tag de {path}"))
        }
    })
}

fn modelo(paths: &[&str], registro: &Arc<Mutex<Registro>>) -> (Controller, Arc<FakePlayer>) {
    let fake = FakePlayer::new();
    let config = Config { load_entry: carregador(registro.clone()), ..config_de_teste() };
    let mut c = Controller::new(fake.clone(), config);
    fake.with(|s| {
        s.itens = itens(paths);
        s.indice_atual = Some(0);
        s.estado = PlayerState::Loaded;
        s.versao_fila += 1;
    });
    c.tick();
    (c, fake)
}

#[test]
fn cada_faixa_que_entra_na_fila_ganha_sua_linha() {
    let registro = Arc::new(Mutex::new(Registro::default()));
    let paths = ["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"];
    let (mut c, _) = modelo(&paths, &registro);

    assert!(c.wait_entries(PRAZO));

    for path in paths {
        assert_eq!(c.model().entries[path].title, format!("tag de {path}"));
    }
}

#[test]
fn nunca_passa_de_quatro_cargas_ao_mesmo_tempo() {
    let registro = Arc::new(Mutex::new(Registro::default()));
    let owned: Vec<String> = (0..12).map(|i| format!("/tmp/faixa-{i}.flac")).collect();
    let paths: Vec<&str> = owned.iter().map(String::as_str).collect();
    let (mut c, _) = modelo(&paths, &registro);

    assert!(c.wait_entries(PRAZO));

    assert_eq!(registro.lock().unwrap().pico, 4);
    assert_eq!(c.model().entries.len(), 12);
}

#[test]
fn o_topo_da_fila_carrega_primeiro() {
    let registro = Arc::new(Mutex::new(Registro::default()));
    let owned: Vec<String> = (0..8).map(|i| format!("/tmp/faixa-{i}.flac")).collect();
    let paths: Vec<&str> = owned.iter().map(String::as_str).collect();
    let (mut c, _) = modelo(&paths, &registro);

    assert!(c.wait_entries(PRAZO));

    // Dentro de um lote de quatro a ordem de partida não é determinística; entre lotes é.
    let ordem = registro.lock().unwrap().ordem.clone();
    let primeiros: HashSet<&str> = ordem[..4].iter().map(String::as_str).collect();
    let ultimos: HashSet<&str> = ordem[4..].iter().map(String::as_str).collect();
    assert_eq!(primeiros, paths[..4].iter().copied().collect::<HashSet<_>>());
    assert_eq!(ultimos, paths[4..].iter().copied().collect::<HashSet<_>>());
}

#[test]
fn tick_sem_mudanca_na_fila_nao_recarrega() {
    let registro = Arc::new(Mutex::new(Registro::default()));
    let (mut c, _) = modelo(&["/tmp/a.flac", "/tmp/b.flac"], &registro);
    assert!(c.wait_entries(PRAZO));

    c.tick();
    c.tick();
    assert!(c.wait_entries(PRAZO));

    assert_eq!(registro.lock().unwrap().ordem.len(), 2);
}

#[test]
fn faixa_nova_carrega_so_ela_mesma() {
    let registro = Arc::new(Mutex::new(Registro::default()));
    let (mut c, fake) = modelo(&["/tmp/a.flac"], &registro);
    assert!(c.wait_entries(PRAZO));

    fake.acrescentar("/tmp/b.flac");
    fake.with(|s| s.versao_fila += 1);
    c.tick();
    assert!(c.wait_entries(PRAZO));

    assert_eq!(registro.lock().unwrap().ordem, ["/tmp/a.flac", "/tmp/b.flac"]);
    assert_eq!(c.model().entries.len(), 2);
}

#[test]
fn limpar_a_fila_descarta_as_linhas() {
    let registro = Arc::new(Mutex::new(Registro::default()));
    let (mut c, _) = modelo(&["/tmp/a.flac", "/tmp/b.flac"], &registro);
    assert!(c.wait_entries(PRAZO));
    assert_eq!(c.model().entries.len(), 2);

    c.clear();
    assert!(c.wait_idle(PRAZO));

    assert!(c.model().entries.is_empty());
}

/// As linhas são por caminho, como no Swift: o mesmo arquivo duas vezes divide a linha, e
/// remover uma das duas não pode apagar os dados da outra.
#[test]
fn a_mesma_faixa_duas_vezes_divide_a_linha_e_remover_uma_nao_apaga_a_outra() {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&["/tmp/a.flac", "/tmp/a.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Loaded;
    });
    let mut c = controlador(&fake);
    assert!(c.wait_entries(PRAZO));
    assert_eq!(c.model().entries.len(), 1);

    c.remove(2);
    assert!(c.wait_idle(PRAZO));

    assert_eq!(c.model().queue.items.len(), 1);
    assert_eq!(c.model().entries["/tmp/a.flac"].title, "fila /tmp/a.flac");
}
```

- [ ] **Step 5: Testes do equivalente do `@Observable` (vermelho)**

`apps/gpui/model/tests/controller_observation.rs`:

```rust
use std::sync::Arc;

use hog_audio::transitions::PlayerState;
use hog_player_model::controller::{Changes, Controller};
use hog_player_testing::{FakePlayer, PRAZO, controlador, itens};

fn tocando() -> (Controller, Arc<FakePlayer>) {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&["/tmp/a.flac", "/tmp/b.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = PlayerState::Playing;
        s.decorrido = 1.0;
    });
    let mut c = controlador(&fake);
    assert!(c.wait_entries(PRAZO));
    assert!(c.wait_metadata(PRAZO));
    c.tick();
    (c, fake)
}

#[test]
fn mudanca_de_estado_marca_o_display() {
    let (mut c, fake) = tocando();
    fake.with(|s| s.estado = PlayerState::Paused);
    assert!(c.tick().display);
}

/// Tocando, o relógio muda a cada tique. Só ele pode ser marcado: capa, título e fila ficam como
/// estão. O tique cruza o segundo, para mudar o texto e a fração tocada.
#[test]
fn tique_do_relogio_so_marca_o_relogio() {
    let (mut c, fake) = tocando();
    fake.with(|s| s.decorrido = 2.0);
    assert_eq!(c.tick(), Changes { clock: true, ..Changes::NONE });
}

#[test]
fn tick_sem_mudanca_nao_marca_nada() {
    let (mut c, _) = tocando();
    assert_eq!(c.tick(), Changes::NONE);
    assert_eq!(c.tick(), Changes::NONE);
}
```

- [ ] **Step 6: Rodar e ver falhar**

Run: `cd apps/gpui && cargo test -p hog-player-model --test controller_transport --test controller_volume --test controller_queue --test controller_observation`
Expected: falha de compilação. O primeiro erro é `unresolved import hog_player_model::controller` no crate `hog-player-testing`.

- [ ] **Step 7: Implementar o modelo**

`apps/gpui/model/src/lib.rs`: acrescentar `pub mod controller;` à lista de módulos, em ordem alfabética.

`apps/gpui/model/src/controller/model.rs`:

```rust
use std::collections::HashMap;

use crate::display::{ClockState, DisplayState};
use crate::entry::QueueEntryInfo;
use crate::port::Track;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueueState {
    pub items: Vec<Track>,
    pub current_index: Option<u32>,
    pub is_playing: bool,
}

impl QueueState {
    pub fn current(&self) -> Option<&Track> {
        self.current_index.and_then(|index| self.items.get(index as usize))
    }

    /// A linha da faixa atual: é por ela que a fila marca a seleção e rola até a atual.
    pub fn current_item_id(&self) -> Option<u64> {
        self.current().map(|track| track.id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeState {
    pub value: f64,
    pub muted: bool,
}

/// O que a tela lê, dividido como no `@Observable` do Swift: cada parte muda sozinha, e o
/// `tick()` diz quais mudaram para a UI redesenhar só elas.
#[derive(Clone, Debug)]
pub struct Model {
    pub display: DisplayState,
    pub clock: ClockState,
    pub queue: QueueState,
    /// Por caminho, como no Swift: chega aos poucos depois que a faixa entra na fila; até lá a
    /// linha mostra o nome do arquivo.
    pub entries: HashMap<String, QueueEntryInfo>,
    pub volume: VolumeState,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub display: bool,
    pub clock: bool,
    pub queue: bool,
    pub entries: bool,
    pub volume: bool,
    pub error: bool,
}

impl Changes {
    pub const NONE: Self = Self {
        display: false,
        clock: false,
        queue: false,
        entries: false,
        volume: false,
        error: false,
    };

    pub fn any(self) -> bool {
        self != Self::NONE
    }
}

/// O que o tique compara antes e depois. A fila e as linhas não entram: comparar mil faixas a
/// cada tique seria caro, e quem as muda já sabe que mudou.
pub(crate) struct Observed {
    display: DisplayState,
    clock: ClockState,
    current_index: Option<u32>,
    is_playing: bool,
    volume: VolumeState,
    error: Option<String>,
}

impl Observed {
    pub(crate) fn capture(model: &Model) -> Self {
        Self {
            display: model.display.clone(),
            clock: model.clock.clone(),
            current_index: model.queue.current_index,
            is_playing: model.queue.is_playing,
            volume: model.volume,
            error: model.error.clone(),
        }
    }

    pub(crate) fn changes(self, model: &Model, items_changed: bool, entries_changed: bool) -> Changes {
        Changes {
            display: self.display != model.display,
            clock: self.clock != model.clock,
            queue: items_changed
                || self.current_index != model.queue.current_index
                || self.is_playing != model.queue.is_playing,
            entries: entries_changed,
            volume: self.volume != model.volume,
            error: self.error != model.error,
        }
    }
}
```

- [ ] **Step 8: Implementar a thread de comandos**

`apps/gpui/model/src/controller/commands.rs`:

```rust
//! A thread de comandos: executa as chamadas bloqueantes ao core na ordem de chegada. Todas
//! podem soltar e tomar o mesmo device, então a ordem tem de ser preservada fora da main.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hog_audio::api::{PlayerError, TrackFormat};
use hog_audio::transitions::PlayerState;

use super::Wake;
use crate::audio_files::audio_files;
use crate::port::{Format, Player, Timings};

pub(crate) const NO_AUDIO_FOUND: &str = "nenhum arquivo de áudio encontrado";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JobKind {
    Toggle,
    Navigate,
    Select,
    Remove,
    Clear,
    Add,
    Advance,
    Recover,
}

pub(crate) enum Job {
    Toggle,
    Navigate,
    Select(u64),
    Remove(u64),
    Clear,
    Add(Vec<PathBuf>),
    Advance,
    Recover,
}

impl Job {
    fn kind(&self) -> JobKind {
        match self {
            Job::Toggle => JobKind::Toggle,
            Job::Navigate => JobKind::Navigate,
            Job::Select(_) => JobKind::Select,
            Job::Remove(_) => JobKind::Remove,
            Job::Clear => JobKind::Clear,
            Job::Add(_) => JobKind::Add,
            Job::Advance => JobKind::Advance,
            Job::Recover => JobKind::Recover,
        }
    }
}

pub(crate) struct Envelope {
    pub job: Job,
    pub queued_at: Instant,
}

/// O que a main e a thread de comandos dividem. O resto é só da main.
pub(crate) struct Shared {
    pub shutting_down: AtomicBool,
    /// Os passos de Next/Previous que ainda esperam a vez. Cliques somam aqui; a thread zera ao
    /// começar, e daí em diante um clique novo abre outra navegação.
    pending_navigation: Mutex<Option<i32>>,
    /// O último `problem_seq` que a main já mostrou; a thread o lê no começo de cada comando.
    pub shown_problem_seq: AtomicU64,
}

impl Shared {
    pub fn new(problem_seq: u64) -> Self {
        Self {
            shutting_down: AtomicBool::new(false),
            pending_navigation: Mutex::new(None),
            shown_problem_seq: AtomicU64::new(problem_seq),
        }
    }

    pub fn navigation(&self) -> MutexGuard<'_, Option<i32>> {
        self.pending_navigation.lock().unwrap_or_else(|p| p.into_inner())
    }
}

pub(crate) struct Report {
    pub command: String,
    pub queued_at: Instant,
    pub started_at: Instant,
    pub finished_at: Instant,
    pub timings: Timings,
    pub problem_seq_at_start: u64,
}

impl Report {
    /// O mesmo formato de linha do Swift (`logTransition`), para os dois logs se compararem.
    pub fn line(&self) -> String {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let t = &self.timings;
        format!(
            "{}: espera {:.0} ms, comando {:.0} ms (open {:.1}, release {:.1}, reopen {:.1}, \
             acquire {:.1}, volume {:.1}, prefill {:.1}, start {:.1})",
            self.command,
            ms(self.started_at - self.queued_at),
            ms(self.finished_at - self.started_at),
            t.open_ms,
            t.release_ms,
            t.reopen_ms,
            t.acquire_ms,
            t.volume_ms,
            t.prefill_ms,
            t.start_ms
        )
    }
}

pub(crate) enum Effect {
    Nothing,
    Toggled { result: Result<(), String>, device_volume: Option<f32> },
    Loaded(Result<Format, String>),
    Advanced(Result<Option<Format>, String>),
    Removed(Result<Option<Format>, String>),
    Cleared(Result<(), String>),
    AddFailed(String),
}

pub(crate) struct Outcome {
    pub kind: JobKind,
    pub effect: Effect,
    pub report: Option<Report>,
}

impl Outcome {
    fn nothing(kind: JobKind) -> Self {
        Self { kind, effect: Effect::Nothing, report: None }
    }
}

pub(crate) fn spawn(
    player: Arc<dyn Player>,
    shared: Arc<Shared>,
    wake: Wake,
) -> (Sender<Envelope>, Receiver<Outcome>) {
    let (jobs, job_rx) = mpsc::channel::<Envelope>();
    let (outcome_tx, outcomes) = mpsc::channel();
    std::thread::Builder::new()
        .name("hog-commands".to_string())
        .spawn(move || {
            while let Ok(envelope) = job_rx.recv() {
                if outcome_tx.send(run(player.as_ref(), &shared, envelope)).is_err() {
                    return;
                }
                wake();
            }
        })
        .expect("não consegui criar a thread de comandos");
    (jobs, outcomes)
}

fn run(player: &dyn Player, shared: &Shared, envelope: Envelope) -> Outcome {
    let kind = envelope.job.kind();
    // A navegação sai da espera antes do guard: daqui em diante, um clique novo abre outra.
    let steps = if kind == JobKind::Navigate { shared.navigation().take().unwrap_or(0) } else { 0 };
    if shared.shutting_down.load(Ordering::Acquire) {
        return Outcome::nothing(kind);
    }
    let queued_at = envelope.queued_at;
    match envelope.job {
        Job::Toggle => toggle(player, shared, queued_at),
        Job::Navigate if steps == 0 => Outcome::nothing(kind),
        Job::Navigate => timed(kind, player, shared, queued_at, format!("navegação {steps}"), || {
            Effect::Loaded(loaded(player.navigate(steps)))
        }),
        Job::Select(id) => timed(kind, player, shared, queued_at, "seleção", || {
            Effect::Loaded(loaded(player.select_track_id(id)))
        }),
        // Removida a atual, o engine já carregou quem assumiu e devolve o formato dela.
        Job::Remove(id) => timed(kind, player, shared, queued_at, "remoção", || {
            Effect::Removed(maybe_loaded(player.remove_track_id(id)))
        }),
        Job::Clear => Outcome {
            kind,
            effect: Effect::Cleared(player.clear_queue().map_err(|e| e.to_string())),
            report: None,
        },
        Job::Add(paths) => add(player, shared, queued_at, &paths),
        Job::Advance => timed(kind, player, shared, queued_at, "auto-avanço", || {
            Effect::Advanced(maybe_loaded(player.advance()))
        }),
        Job::Recover => {
            // A única falha possível é o player já estar encerrando.
            let _ = player.recover();
            Outcome::nothing(kind)
        }
    }
}

/// Decide entre tocar e pausar pelo estado lido na hora de executar, não na hora do clique:
/// dois toggles rápidos tocam e depois pausam.
fn toggle(player: &dyn Player, shared: &Shared, queued_at: Instant) -> Outcome {
    let state = player.snapshot().state;
    let pause = state == PlayerState::Playing;
    let play = matches!(state, PlayerState::Loaded | PlayerState::Paused | PlayerState::Finished);
    if !pause && !play {
        return Outcome::nothing(JobKind::Toggle);
    }
    let command = if pause { "pause" } else { "play" };
    timed(JobKind::Toggle, player, shared, queued_at, command, || {
        let result = if pause { player.pause() } else { player.play() };
        // Só o play publica o volume real (o teto de segurança pode ter agido): a tela relê.
        Effect::Toggled {
            result: result.map_err(|e| e.to_string()),
            device_volume: player.snapshot().volume_scalar,
        }
    })
}

fn add(player: &dyn Player, shared: &Shared, queued_at: Instant, paths: &[PathBuf]) -> Outcome {
    let was_empty = player.snapshot().queue_len == 0;
    let files = audio_files(paths);
    if files.is_empty() {
        return Outcome {
            kind: JobKind::Add,
            effect: Effect::AddFailed(NO_AUDIO_FOUND.to_string()),
            report: None,
        };
    }
    player.append_tracks(files.iter().map(|p| p.to_string_lossy().into_owned()).collect());
    if !was_empty {
        return Outcome::nothing(JobKind::Add);
    }
    // `append_tracks` não propaga a falha da carga inicial; selecionar a posição 0 torna o erro
    // visível.
    timed(JobKind::Add, player, shared, queued_at, "carga inicial", || {
        Effect::Loaded(loaded(player.select_track(0)))
    })
}

fn timed(
    kind: JobKind,
    player: &dyn Player,
    shared: &Shared,
    queued_at: Instant,
    command: impl Into<String>,
    work: impl FnOnce() -> Effect,
) -> Outcome {
    let problem_seq_at_start = shared.shown_problem_seq.load(Ordering::Acquire);
    let started_at = Instant::now();
    let effect = work();
    let finished_at = Instant::now();
    Outcome {
        kind,
        effect,
        report: Some(Report {
            command: command.into(),
            queued_at,
            started_at,
            finished_at,
            timings: player.last_timings().into(),
            problem_seq_at_start,
        }),
    }
}

fn loaded(result: Result<TrackFormat, PlayerError>) -> Result<Format, String> {
    result.map(|format| Format::from(&format)).map_err(|e| e.to_string())
}

fn maybe_loaded(result: Result<Option<TrackFormat>, PlayerError>) -> Result<Option<Format>, String> {
    result
        .map(|format| format.as_ref().map(Format::from))
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 9: Implementar o worker de volume**

`apps/gpui/model/src/controller/volume.rs`:

```rust
//! Um escritor só, com o pedido mais recente sobrescrevendo o anterior. A ordem é garantida por
//! construção, e um arrasto inteiro custa poucas escritas em vez de uma por quadro: o hardware
//! precisa do valor em que o dedo parou, não dos intermediários.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use super::Wake;
use crate::port::Player;

#[derive(Default)]
struct Slot {
    desired: Option<(u64, f32)>,
    stop: bool,
}

#[derive(Default)]
struct Shared {
    slot: Mutex<Slot>,
    wake: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(|p| p.into_inner())
    }
}

pub(crate) enum VolumeResult {
    Written,
    Failed { message: String, device_volume: Option<f32> },
}

struct Settled {
    through: u64,
    result: VolumeResult,
}

pub(crate) struct VolumeWorker {
    shared: Arc<Shared>,
    results: Receiver<Settled>,
    requested: u64,
    settled: u64,
}

impl VolumeWorker {
    pub fn spawn(player: Arc<dyn Player>, wake: Wake) -> Self {
        let shared = Arc::new(Shared::default());
        let (tx, results) = mpsc::channel();
        let worker = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("hog-volume".to_string())
            .spawn(move || {
                while let Some((seq, value)) = next_request(&worker) {
                    let settled = match player.set_volume(value) {
                        Ok(()) => Settled { through: seq, result: VolumeResult::Written },
                        Err(error) => {
                            // O resto do arrasto sai junto: insistir por cima de uma recusa só
                            // repetiria o erro.
                            let dropped = worker.lock().desired.take().map_or(seq, |(later, _)| later);
                            Settled {
                                through: dropped.max(seq),
                                result: VolumeResult::Failed {
                                    message: error.to_string(),
                                    device_volume: player.snapshot().volume_scalar,
                                },
                            }
                        }
                    };
                    if tx.send(settled).is_err() {
                        return;
                    }
                    wake();
                }
            })
            .expect("não consegui criar a thread de volume");
        Self { shared, results, requested: 0, settled: 0 }
    }

    pub fn request(&mut self, value: f32) {
        self.requested += 1;
        self.shared.lock().desired = Some((self.requested, value));
        self.shared.wake.notify_one();
    }

    /// Há pedido que ainda não voltou: esperando, sendo escrito ou com a resposta no canal.
    pub fn busy(&self) -> bool {
        self.settled != self.requested
    }

    pub fn drain(&mut self) -> Vec<VolumeResult> {
        let mut results = Vec::new();
        while let Ok(settled) = self.results.try_recv() {
            self.settled = self.settled.max(settled.through);
            results.push(settled.result);
        }
        results
    }

    pub fn stop(&self) {
        self.shared.lock().stop = true;
        self.shared.wake.notify_one();
    }
}

fn next_request(shared: &Shared) -> Option<(u64, f32)> {
    let mut slot = shared.lock();
    loop {
        if slot.stop {
            return None;
        }
        if let Some(request) = slot.desired.take() {
            return Some(request);
        }
        slot = shared.wake.wait(slot).unwrap_or_else(|p| p.into_inner());
    }
}
```

- [ ] **Step 10: Implementar as cargas**

`apps/gpui/model/src/controller/loaders.rs`:

```rust
//! As cargas fora da main: as linhas da fila, em quatro threads, e os metadados completos da
//! faixa atual, fora da cadeia de comandos.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use super::{EntryLoad, MetadataLoad, Wake};
use crate::entry::QueueEntryInfo;
use crate::metadata::TrackMetadata;
use crate::port::Track;

/// Soltar uma pasta de mil faixas não pode disparar mil leituras de tag e de capa ao mesmo
/// tempo: quatro enchem a lista de cima para baixo sem disputar CPU e disco com a reprodução.
const ENTRY_WORKERS: usize = 4;

#[derive(Default)]
struct Pending {
    paths: VecDeque<String>,
    stop: bool,
}

#[derive(Default)]
struct EntryShared {
    pending: Mutex<Pending>,
    wake: Condvar,
}

impl EntryShared {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(|p| p.into_inner())
    }
}

pub(crate) struct EntryLoader {
    shared: Arc<EntryShared>,
    results: Receiver<(String, QueueEntryInfo)>,
    /// Pedidos que ainda não voltaram: na espera, carregando ou com o resultado no canal.
    requested: HashSet<String>,
}

impl EntryLoader {
    pub fn spawn(load: EntryLoad, wake: Wake) -> Self {
        let shared = Arc::new(EntryShared::default());
        let (tx, results) = mpsc::channel();
        for index in 0..ENTRY_WORKERS {
            let (shared, load, tx) = (Arc::clone(&shared), Arc::clone(&load), tx.clone());
            let wake = Arc::clone(&wake);
            std::thread::Builder::new()
                .name(format!("hog-entry-{index}"))
                .spawn(move || {
                    while let Some(path) = next_path(&shared) {
                        let info = load(&path);
                        if tx.send((path, info)).is_err() {
                            return;
                        }
                        wake();
                    }
                })
                .expect("não consegui criar a thread de carga");
        }
        Self { shared, results, requested: HashSet::new() }
    }

    /// Alinha a carga com a fila: descarta o que saiu e pede, na ordem da fila, o que falta. O
    /// topo visível enche primeiro. Devolve se alguma linha saiu.
    pub fn sync(&mut self, queue: &[Track], entries: &mut HashMap<String, QueueEntryInfo>) -> bool {
        let queued: HashSet<&str> = queue.iter().map(|track| track.path.as_str()).collect();
        let before = entries.len();
        entries.retain(|path, _| queued.contains(path.as_str()));
        self.requested.retain(|path| queued.contains(path.as_str()));
        let mut pending = self.shared.lock();
        pending.paths.retain(|path| queued.contains(path.as_str()));
        for track in queue {
            if !entries.contains_key(&track.path) && self.requested.insert(track.path.clone()) {
                pending.paths.push_back(track.path.clone());
            }
        }
        drop(pending);
        self.shared.wake.notify_all();
        entries.len() != before
    }

    /// A faixa pode ter saído da fila enquanto carregava: só vale o que ainda está pedido.
    pub fn drain(&mut self, entries: &mut HashMap<String, QueueEntryInfo>) -> bool {
        let mut changed = false;
        while let Ok((path, info)) = self.results.try_recv() {
            if self.requested.remove(&path) {
                entries.insert(path, info);
                changed = true;
            }
        }
        changed
    }

    pub fn idle(&self) -> bool {
        self.requested.is_empty()
    }

    pub fn stop(&mut self) {
        let mut pending = self.shared.lock();
        pending.stop = true;
        pending.paths.clear();
        drop(pending);
        self.shared.wake.notify_all();
        self.requested.clear();
    }
}

fn next_path(shared: &EntryShared) -> Option<String> {
    let mut pending = shared.lock();
    loop {
        if pending.stop {
            return None;
        }
        if let Some(path) = pending.paths.pop_front() {
            return Some(path);
        }
        pending = shared.wake.wait(pending).unwrap_or_else(|p| p.into_inner());
    }
}

type MetadataResult = (u64, String, TrackMetadata);

/// Os metadados completos da faixa atual, com a capa grande. Cada pedido tem uma geração; o
/// resultado de uma geração antiga é descartado, porque outra faixa assumiu enquanto ele lia.
pub(crate) struct MetadataLoader {
    load: MetadataLoad,
    wake: Wake,
    generation: u64,
    tx: Sender<MetadataResult>,
    results: Receiver<MetadataResult>,
    running: Arc<AtomicUsize>,
}

impl MetadataLoader {
    pub fn new(load: MetadataLoad, wake: Wake) -> Self {
        let (tx, results) = mpsc::channel();
        Self { load, wake, generation: 0, tx, results, running: Arc::new(AtomicUsize::new(0)) }
    }

    pub fn request(&mut self, path: String) {
        self.generation += 1;
        let generation = self.generation;
        let (load, tx, running) = (Arc::clone(&self.load), self.tx.clone(), Arc::clone(&self.running));
        let wake = Arc::clone(&self.wake);
        running.fetch_add(1, Ordering::AcqRel);
        std::thread::Builder::new()
            .name("hog-metadata".to_string())
            .spawn(move || {
                let metadata = load(&path);
                // Envia antes de baixar o contador: quando `idle()` vale, o resultado já está no
                // canal.
                let _ = tx.send((generation, path, metadata));
                running.fetch_sub(1, Ordering::AcqRel);
                wake();
            })
            .expect("não consegui criar a thread de metadados");
    }

    pub fn cancel(&mut self) {
        self.generation += 1;
    }

    pub fn drain(&mut self) -> Option<(String, TrackMetadata)> {
        let mut latest = None;
        while let Ok((generation, path, metadata)) = self.results.try_recv() {
            if generation == self.generation {
                latest = Some((path, metadata));
            }
        }
        latest
    }

    pub fn idle(&self) -> bool {
        self.running.load(Ordering::Acquire) == 0
    }
}
```

- [ ] **Step 11: Implementar o `Controller`**

`apps/gpui/model/src/controller/mod.rs`:

```rust
//! O controlador: porte das regras do `PlayerViewModel.swift`, sem nada de UI.
//!
//! Só quem chama `tick()` (a main, no app) mexe no modelo. Os comandos bloqueantes rodam em
//! série numa thread própria, e o volume e as cargas têm threads próprias. As respostas voltam
//! por canal e são aplicadas no tique seguinte, como a main actor fazia no Swift.

mod commands;
mod loaders;
mod model;
mod volume;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hog_audio::api::Snapshot;
use hog_audio::transitions::PlayerState;

use crate::display::{clock_state, display_state};
use crate::entry::{QueueEntryInfo, load_queue_entry};
use crate::media::tags::file_stem;
use crate::metadata::{TrackMetadata, load_track_metadata};
use crate::port::{Format, Player, Track};
use commands::{Effect, Envelope, Job, JobKind, Outcome, Shared};
use loaders::{EntryLoader, MetadataLoader};
use model::Observed;
use volume::{VolumeResult, VolumeWorker};

pub use model::{Changes, Model, QueueState, VolumeState};

pub type EntryLoad = Arc<dyn Fn(&str) -> QueueEntryInfo + Send + Sync>;
pub type MetadataLoad = Arc<dyn Fn(&str) -> TrackMetadata + Send + Sync>;
pub type LogSink = Arc<dyn Fn(&str) + Send + Sync>;
/// Chamado por qualquer thread do controlador ao entregar uma resposta. O app acorda a main para
/// um tique na hora, como o `apply` do Swift, que chamava `refresh()` sem esperar o timer.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// A mesma tolerância da conferência de escrita do engine: abaixo dela é o volume que nós mesmos
/// pedimos, só que ida e volta por `f32`.
const VOLUME_TOLERANCE: f64 = 0.005;
const INITIAL_VOLUME: f64 = 0.5;

pub struct Config {
    pub load_entry: EntryLoad,
    pub load_metadata: MetadataLoad,
    pub log: LogSink,
    pub wake: Wake,
}

impl Config {
    pub fn production(log: LogSink, wake: Wake) -> Self {
        Self {
            load_entry: Arc::new(load_queue_entry),
            load_metadata: Arc::new(load_track_metadata),
            log,
            wake,
        }
    }
}

pub struct Controller {
    player: Arc<dyn Player>,
    shared: Arc<Shared>,
    jobs: Sender<Envelope>,
    outcomes: Receiver<Outcome>,
    volume: VolumeWorker,
    entries: EntryLoader,
    metadata: MetadataLoader,
    log: LogSink,
    model: Model,
    format: Option<Format>,
    /// Os metadados completos e de qual faixa eles são.
    current_metadata: Option<TrackMetadata>,
    metadata_path: Option<String>,
    reload_metadata: bool,
    last_queue_version: u64,
    last_problem_seq: u64,
    in_flight: usize,
    advancing: bool,
    advanced_finished_index: Option<u32>,
    recovering: bool,
    volume_editing: bool,
    shutting_down: bool,
}

impl Controller {
    pub fn new(player: Arc<dyn Player>, config: Config) -> Self {
        let snapshot = player.snapshot();
        let items: Vec<Track> = player.queue_items().into_iter().map(Track::from).collect();
        let shared = Arc::new(Shared::new(snapshot.problem_seq));
        let wake = config.wake;
        let (jobs, outcomes) =
            commands::spawn(Arc::clone(&player), Arc::clone(&shared), Arc::clone(&wake));
        let mut controller = Self {
            volume: VolumeWorker::spawn(Arc::clone(&player), Arc::clone(&wake)),
            entries: EntryLoader::spawn(config.load_entry, Arc::clone(&wake)),
            metadata: MetadataLoader::new(config.load_metadata, wake),
            log: config.log,
            model: Model {
                display: display_state(&snapshot, None, None),
                clock: clock_state(&snapshot),
                queue: QueueState {
                    items,
                    current_index: snapshot.current_index,
                    is_playing: snapshot.state == PlayerState::Playing,
                },
                entries: HashMap::new(),
                volume: VolumeState { value: INITIAL_VOLUME, muted: false },
                error: None,
            },
            format: None,
            current_metadata: None,
            metadata_path: None,
            reload_metadata: false,
            last_queue_version: snapshot.queue_version,
            // Um problema de antes de a janela abrir não é notícia.
            last_problem_seq: snapshot.problem_seq,
            in_flight: 0,
            advancing: false,
            advanced_finished_index: None,
            recovering: false,
            volume_editing: false,
            shutting_down: false,
            player,
            shared,
            jobs,
            outcomes,
        };
        controller.entries.sync(&controller.model.queue.items, &mut controller.model.entries);
        controller
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// O poll: dez vezes por segundo no app. Aplica as respostas, lê o snapshot (só atômicos no
    /// core) e dispara recuperação e auto-avanço. Devolve quais partes do modelo mudaram.
    pub fn tick(&mut self) -> Changes {
        let before = Observed::capture(&self.model);
        let mut items_changed = false;

        while let Ok(outcome) = self.outcomes.try_recv() {
            self.apply(outcome);
        }
        self.apply_volume_results();
        let mut entries_changed = self.entries.drain(&mut self.model.entries);
        self.accept_metadata();

        let snapshot = self.player.snapshot();
        self.model.queue.current_index = snapshot.current_index;
        // `try_queue_items` nunca espera o engine: com um comando segurando o lock (uma troca de
        // device leva segundos), a versão não avança e o próximo tique tenta de novo.
        if self.last_queue_version != snapshot.queue_version
            && let Some(items) = self.player.try_queue_items()
        {
            let items: Vec<Track> = items.into_iter().map(Track::from).collect();
            if items != self.model.queue.items {
                self.model.queue.items = items;
                items_changed = true;
            }
            self.last_queue_version = snapshot.queue_version;
            entries_changed |= self.entries.sync(&self.model.queue.items, &mut self.model.entries);
        }
        if snapshot.current_index.is_none() {
            self.forget_current_track();
        }
        if std::mem::take(&mut self.reload_metadata) {
            self.reload_metadata_for_current_track();
        }

        self.model.queue.is_playing = snapshot.state == PlayerState::Playing;
        let shown = self.shown_metadata();
        self.model.display = display_state(&snapshot, shown.as_ref(), self.format.as_ref());
        self.model.clock = clock_state(&snapshot);
        self.model.volume.muted = snapshot.muted;
        self.follow_device_volume(&snapshot);
        self.show_new_problem(&snapshot);

        // Antes do auto-avanço: a recuperação conta como comando em curso, e é isso que impede
        // tocar a faixa seguinte no device que o macOS pôs no lugar do perdido.
        if snapshot.device_suspect && !self.recovering && !self.shutting_down {
            self.recovering = true;
            self.enqueue(Job::Recover);
        }
        if snapshot.state != PlayerState::Finished {
            self.advanced_finished_index = None;
        }
        if let Some(index) = self.finished_index_to_advance(&snapshot) {
            self.advancing = true;
            self.advanced_finished_index = Some(index);
            self.enqueue(Job::Advance);
        }

        before.changes(&self.model, items_changed, entries_changed)
    }

    pub fn toggle(&mut self) {
        self.enqueue(Job::Toggle);
    }

    pub fn next(&mut self) {
        self.navigate(1);
    }

    pub fn previous(&mut self) {
        self.navigate(-1);
    }

    /// Por id, não por índice: a fila da tela pode estar defasada em relação à do engine, e o
    /// índice de uma linha muda a cada remoção.
    pub fn select(&mut self, id: u64) {
        self.enqueue(Job::Select(id));
    }

    pub fn remove(&mut self, id: u64) {
        self.enqueue(Job::Remove(id));
    }

    pub fn clear(&mut self) {
        self.enqueue(Job::Clear);
    }

    /// Expandir pastas roda na thread de comandos: uma pasta grande leva segundos.
    pub fn add(&mut self, paths: Vec<PathBuf>) {
        self.enqueue(Job::Add(paths));
    }

    /// Publica na hora: o slider não pode esperar o mutex do core, o mesmo que um play segura
    /// durante a aquisição do hog.
    pub fn apply_volume(&mut self, scalar: f64) {
        if self.shutting_down {
            return;
        }
        self.model.volume.value = scalar;
        self.volume.request(scalar as f32);
    }

    pub fn set_volume_editing(&mut self, editing: bool) {
        self.volume_editing = editing;
    }

    /// Descarta o que ainda não começou e devolve o device. O `shutdown` do core espera o
    /// comando em curso pelo lock de comandos; a falha volta para quem chamou registrar, porque
    /// a janela já está fechando.
    pub fn shutdown(&mut self) -> Result<(), String> {
        if self.shutting_down {
            return Ok(());
        }
        self.shutting_down = true;
        self.shared.shutting_down.store(true, Ordering::Release);
        self.entries.stop();
        self.metadata.cancel();
        self.player.shutdown().map_err(|error| error.to_string())
    }

    pub fn is_idle(&self) -> bool {
        self.in_flight == 0 && !self.volume.busy()
    }

    pub fn wait_idle(&mut self, timeout: Duration) -> bool {
        self.wait_until(timeout, Self::is_idle)
    }

    pub fn wait_entries(&mut self, timeout: Duration) -> bool {
        self.wait_until(timeout, |c| c.entries.idle())
    }

    pub fn wait_metadata(&mut self, timeout: Duration) -> bool {
        self.wait_until(timeout, |c| c.metadata.idle())
    }

    /// Tique até `done` valer, com teto. Existe para os testes e para o encerramento; a UI normal
    /// só chama `tick()`.
    pub fn wait_until(&mut self, timeout: Duration, done: impl Fn(&Self) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            self.tick();
            if done(self) {
                self.tick();
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Cinco cliques rápidos em Next eram cinco cargas completas, cada uma soltando e tomando o
    /// device. Enquanto a navegação ainda espera a vez, cliques novos só somam passos nela.
    fn navigate(&mut self, delta: i32) {
        if self.shutting_down {
            return;
        }
        {
            let mut pending = self.shared.navigation();
            if let Some(steps) = pending.as_mut() {
                *steps += delta;
                return;
            }
            *pending = Some(delta);
        }
        self.enqueue(Job::Navigate);
    }

    fn enqueue(&mut self, job: Job) {
        if self.shutting_down {
            return;
        }
        self.in_flight += 1;
        if self.jobs.send(Envelope { job, queued_at: Instant::now() }).is_err() {
            self.in_flight -= 1;
        }
    }

    fn apply(&mut self, outcome: Outcome) {
        self.in_flight = self.in_flight.saturating_sub(1);
        match outcome.kind {
            JobKind::Advance => self.advancing = false,
            JobKind::Recover => self.recovering = false,
            _ => {}
        }
        let at_start = outcome.report.as_ref().map(|report| report.problem_seq_at_start);
        if let Some(report) = &outcome.report {
            (self.log)(&report.line());
        }
        match outcome.effect {
            Effect::Nothing | Effect::Advanced(Ok(None)) => {}
            Effect::Toggled { result, device_volume } => {
                match result {
                    Ok(()) => self.clear_error_from_before(at_start),
                    Err(message) => self.model.error = Some(message),
                }
                if let Some(scalar) = device_volume {
                    self.model.volume.value = f64::from(scalar);
                }
            }
            Effect::Loaded(Ok(format))
            | Effect::Advanced(Ok(Some(format)))
            | Effect::Removed(Ok(Some(format))) => {
                self.format = Some(format);
                self.clear_error_from_before(at_start);
                // Não espera os metadados: o título aparece na hora pela linha da fila e a capa
                // chega quando a leitura terminar.
                self.reload_metadata = true;
            }
            Effect::Removed(Ok(None)) => self.clear_error_from_before(at_start),
            Effect::Cleared(Ok(())) => {
                self.forget_current_track();
                self.model.error = None;
            }
            Effect::Loaded(Err(message))
            | Effect::Advanced(Err(message))
            | Effect::Removed(Err(message))
            | Effect::Cleared(Err(message))
            | Effect::AddFailed(message) => self.model.error = Some(message),
        }
    }

    /// Um comando bem-sucedido limpa o erro de antes dele, mas não um problema que o engine
    /// publicou durante o próprio comando e um tique já mostrou.
    fn clear_error_from_before(&mut self, at_start: Option<u64>) {
        if at_start.is_none_or(|seq| seq == self.last_problem_seq) {
            self.model.error = None;
        }
    }

    fn apply_volume_results(&mut self) {
        for result in self.volume.drain() {
            match result {
                VolumeResult::Written => self.model.error = None,
                VolumeResult::Failed { message, device_volume } => {
                    // O slider não pode continuar exibindo um pedido que o device recusou.
                    self.model.error = Some(message);
                    if let Some(scalar) = device_volume {
                        self.model.volume.value = f64::from(scalar);
                    }
                }
            }
        }
    }

    /// O slider segue o volume mudado por fora só quando nada nosso mexe nele. Fora de Playing e
    /// Paused, o device está sendo trocado ou já não é nosso. Durante um arrasto, uma escrita ou
    /// um comando, o hardware ainda tem o valor de antes.
    fn follow_device_volume(&mut self, snapshot: &Snapshot) {
        let Some(scalar) = snapshot.volume_scalar else { return };
        let ours = matches!(snapshot.state, PlayerState::Playing | PlayerState::Paused);
        if !ours || self.volume_editing || self.volume.busy() || self.in_flight > 0 {
            return;
        }
        let device = f64::from(scalar);
        if (device - self.model.volume.value).abs() > VOLUME_TOLERANCE {
            self.model.volume.value = device;
        }
    }

    fn show_new_problem(&mut self, snapshot: &Snapshot) {
        if snapshot.problem_seq == self.last_problem_seq {
            return;
        }
        self.last_problem_seq = snapshot.problem_seq;
        self.shared.shown_problem_seq.store(snapshot.problem_seq, Ordering::Release);
        if let Some(problem) = self.player.last_problem() {
            self.model.error = Some(problem);
        }
    }

    fn finished_index_to_advance(&self, snapshot: &Snapshot) -> Option<u32> {
        if self.shutting_down
            || self.advancing
            || self.in_flight > 0
            || snapshot.state != PlayerState::Finished
        {
            return None;
        }
        let current = snapshot.current_index?;
        (current + 1 < snapshot.queue_len && self.advanced_finished_index != Some(current))
            .then_some(current)
    }

    /// Outra faixa pode ter assumido enquanto esta lia: o resultado dela não vale mais.
    fn accept_metadata(&mut self) {
        let Some((path, metadata)) = self.metadata.drain() else { return };
        if self.model.queue.current().is_some_and(|track| track.path == path) {
            self.current_metadata = Some(metadata);
            self.metadata_path = Some(path);
        }
    }

    fn reload_metadata_for_current_track(&mut self) {
        let Some(path) = self.model.queue.current().map(|track| track.path.clone()) else {
            self.forget_current_track();
            return;
        };
        // Previous que rebobina recarrega a mesma faixa: os metadados já são dela.
        if self.metadata_path.as_deref() == Some(path.as_str()) {
            return;
        }
        self.metadata.request(path);
    }

    /// Sem faixa atual não há o que mostrar: sem isto a tela ficaria com a capa e o formato da
    /// faixa removida.
    fn forget_current_track(&mut self) {
        self.current_metadata = None;
        self.metadata_path = None;
        self.format = None;
        self.metadata.cancel();
    }

    /// Logo depois de uma troca os metadados completos ainda não chegaram. O título vem da linha
    /// da fila (ou do nome do arquivo), e a capa da faixa anterior fica até a nova chegar, em vez
    /// de piscar o placeholder.
    fn shown_metadata(&self) -> Option<TrackMetadata> {
        let Some(track) = self.model.queue.current() else {
            return self.current_metadata.clone();
        };
        if self.metadata_path.as_deref() == Some(track.path.as_str()) {
            return self.current_metadata.clone();
        }
        let entry = self.model.entries.get(&track.path);
        Some(TrackMetadata {
            title: entry.map_or_else(|| file_stem(&track.path), |e| e.title.clone()),
            artist: entry.and_then(|e| e.artist.clone()),
            album: entry.and_then(|e| e.album.clone()),
            artwork: self.current_metadata.as_ref().and_then(|m| m.artwork.clone()),
        })
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.entries.stop();
        self.volume.stop();
    }
}
```

- [ ] **Step 12: Rodar e ver passar**

Run: `cd apps/gpui && cargo test -p hog-player-model`
Expected: todos os testes do crate passam. São cerca de 45 do controlador mais os das Tasks 1 a 3.

Rodar mais duas vezes (`for i in 1 2; do cargo test -p hog-player-model -q || break; done`). Os testes usam threads reais, e um teste instável aqui é defeito, não azar: investigar antes de seguir.

- [ ] **Step 13: Sabotagens (uma por vez, desfazendo com Edit)**

- Em `commands::run`, tirar o `if shared.shutting_down ...`. Reprova `shutdown_descarta_o_que_ainda_nao_chegou_ao_player`.
- Em `commands::spawn`, tirar o `wake();`. Reprova `cada_resposta_acorda_quem_desenha`.
- Em `Controller::navigate`, tirar o `return` de dentro do `if let Some(steps)`. Reprova `tres_nexts_rapidos_viram_uma_carga_so`.
- Em `finished_index_to_advance`, tirar `self.advancing ||` e `self.in_flight > 0 ||`. Reprova `finished_dispara_um_unico_avanco_enquanto_a_carga_corre` ou `next_manual_pendente_impede_auto_avanco_do_mesmo_finished`.
- Em `tick`, mover o bloco da recuperação para depois do auto-avanço. Reprova `device_suspeito_no_fim_da_faixa_recupera_e_nao_avanca`.
- Tirar `!self.recovering &&`. Reprova `suspeita_repetida_no_tick_nao_enfileira_outra_recuperacao`.
- Em `commands::toggle`, trocar `device_volume: player.snapshot().volume_scalar` por `device_volume: None`. Reprova `o_toggle_rele_o_volume_real_do_device_ao_fim_do_comando`.
- Em `clear_error_from_before`, trocar o corpo por `self.model.error = None`. Reprova `aviso_publicado_no_play_nao_some_quando_o_play_termina`.
- Em `follow_device_volume`, tirar `self.volume.busy() ||`. Reprova `o_slider_nao_volta_enquanto_a_escrita_nao_chegou_ao_device`.
- Tirar `self.in_flight > 0` do mesmo `if`. Reprova `o_slider_nao_pisca_na_troca_de_faixa`.
- Em `VolumeWorker::request`, trocar a sobrescrita por uma fila que guarda todos os pedidos (`VecDeque<(u64, f32)>` no `Slot`). Reprova `o_arrasto_do_slider_nao_vira_uma_escrita_por_quadro`.
- Trocar `ENTRY_WORKERS` por `5`. Reprova `nunca_passa_de_quatro_cargas_ao_mesmo_tempo`.
- Em `MetadataLoader::drain`, tirar o `if generation == self.generation`. Reprova `metadado_atrasado_de_faixa_antiga_e_descartado`.
- Em `Observed::changes`, trocar `clock: self.clock != model.clock` por `clock: false` e acrescentar `|| self.clock != model.clock` ao `display`. Reprova `tique_do_relogio_so_marca_o_relogio`.

- [ ] **Step 14: Gate**

Run: `make gpui-fmt && make gpui-test && make test`
Expected: tudo verde.

- [ ] **Step 15: Ponto de parada**

Mostrar `git status --short` e o resumo ao usuário. Commit só se ele pedir, com `feat(gpui): portar o controlador do ViewModel com testes`. São 12 arquivos, abaixo do limite de 20.

---

## Tarefas de UI (5 a 10)

Regras que valem para todas:
- **API.** Os nomes seguem `docs/research/2026-09-27-gpui-v1.21-api.md`. Se um nome não compilar, conferir no relatório e no código da tag antes de mudar o desenho, e registrar a diferença no relatório.
- **Sinais.** Cada view observa só o sinal da parte do modelo que ela lê (`Store::signals`) e entra na raiz com `.cached(...)` e tamanho definido. Um tique do relógio redesenha só o relógio: é o equivalente do `@Observable`.
- **Estado.** Nenhuma view guarda estado de reprodução. Tudo vem de `store.read(cx).model()`, e toda intenção vai por um método do `Store`.
- **Testes.** Os testes de UI usam `TestAppContext`, se a Task 0 provou que o `test-support` funciona. Se não provou, cada teste vira um item de verificação manual no Step de verificação da tarefa, com screenshot.

### Task 5: Store, janela, ações e a coluna da faixa atual

**Files:**
- Modify: `apps/gpui/Cargo.toml` (`futures = "0.3.34"` em `[workspace.dependencies]`)
- Modify: `apps/gpui/app/Cargo.toml`
- Modify: `apps/gpui/app/src/lib.rs`
- Create: `apps/gpui/app/src/theme.rs`, `apps/gpui/app/src/store.rs`, `apps/gpui/app/src/actions.rs`
- Create: `apps/gpui/app/src/views/mod.rs`, `icons.rs`, `tooltip.rs`, `root.rs`, `cover.rs`, `track_info.rs`, `clock.rs`, `transport.rs`, `technical.rs`, `status.rs`
- Modify: `apps/gpui/app/src/main.rs` (substitui o spike)
- Test: `apps/gpui/app/tests/views.rs`

**Interfaces:**
- Consumes:
  - `Controller`, `Config`, `Changes`, `Model` e `Wake` (Task 4);
  - `layout::*` (Task 1);
  - o `FakePlayer` e o `config_de_teste` (Task 4, crate `hog-player-testing`).
- Produces:
  - `store::Store`:
    - `new(Arc<dyn Player>, Config, &mut Context<Self>)`, `start(UnboundedReceiver<()>, cx)`, `tick(cx)` e `model()`;
    - seleção: `selected()`, `play_row(u64, cx)`, `select_row(Option<u64>, cx)`, `move_selection(i32, cx)`, `remove_selected(cx)`;
    - transporte e fila: `toggle()`, `next()`, `previous()`, `clear()`, `add(Vec<PathBuf>)`;
    - volume: `apply_volume(f64, cx)`, `nudge_volume(f64, cx)`, `set_volume_editing(bool)`;
    - encerramento e testes: `shutdown() -> Result<(), String>`, `quit_flag()`, `wait_idle(Duration)`, `wait_entries(Duration)`.
  - `store::{Signal, Signals, wake_channel() -> (Wake, UnboundedReceiver<()>)}`.
  - `actions::{Quit, Hide, Minimize, Open, PlayPause, RemoveSelected, SelectPrevious, SelectNext, bind, register, menus}`.
  - `views::root::RootView::new(Entity<Store>, &mut Window, &mut Context<Self>)`.
  - `views::observe` e `theme::*`.

- [ ] **Step 1: Dependências**

`apps/gpui/Cargo.toml`: acrescentar em `[workspace.dependencies]` a linha `futures = "0.3.34"`.

`apps/gpui/app/Cargo.toml`: `[dependencies]` e `[dev-dependencies]` passam a ser

```toml
[dependencies]
futures.workspace = true
gpui.workspace = true
gpui_platform.workspace = true
hog-audio.workspace = true
hog-player-model = { path = "../model" }
image.workspace = true
signal-hook.workspace = true

[dev-dependencies]
gpui = { workspace = true, features = ["test-support"] }
hog-player-testing = { path = "../testing" }
```

- [ ] **Step 2: Testes (vermelho)**

`apps/gpui/app/tests/views.rs` passa a ser:

```rust
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AppContext, Entity, TestAppContext};
use hog_audio::transitions::PlayerState;
use hog_player_gpui::actions;
use hog_player_gpui::store::Store;
use hog_player_gpui::views::root::RootView;
use hog_player_testing::{FakePlayer, PRAZO, config_de_teste, itens, pasta_com};

#[gpui::test]
fn o_test_support_do_gpui_funciona_fora_do_zed(cx: &mut TestAppContext) {
    let contador = cx.new(|_| 0_u32);
    contador.update(cx, |n, _| *n += 1);
    assert_eq!(contador.read_with(cx, |n, _| *n), 1);
}

fn carregada(estado: PlayerState) -> Arc<FakePlayer> {
    let fake = FakePlayer::new();
    fake.with(|s| {
        s.itens = itens(&["/tmp/a.flac", "/tmp/b.flac", "/tmp/c.flac"]);
        s.indice_atual = Some(0);
        s.versao_fila = 1;
        s.estado = estado;
        s.decorrido = 1.0;
    });
    fake
}

fn store(cx: &mut TestAppContext, fake: &Arc<FakePlayer>) -> Entity<Store> {
    let player = fake.clone();
    cx.new(move |cx| Store::new(player, config_de_teste(), cx))
}

/// Um tique que só avança o relógio notifica só o sinal do relógio: é o que deixa as outras
/// views em cache.
#[gpui::test]
fn o_tique_do_relogio_notifica_so_o_sinal_do_relogio(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Playing);
    let store = store(cx, &fake);
    // As linhas carregando depois mudariam o título mostrado e sujariam a contagem.
    store.update(cx, |s, _| assert!(s.wait_idle(PRAZO) && s.wait_entries(PRAZO)));
    store.update(cx, |s, cx| s.tick(cx));

    let relogio = Rc::new(Cell::new(0));
    let tela = Rc::new(Cell::new(0));
    let fila = Rc::new(Cell::new(0));
    cx.update(|cx| {
        let signals = store.read(cx).signals.clone();
        for (sinal, conta) in [(&signals.clock, &relogio), (&signals.display, &tela), (&signals.queue, &fila)] {
            let conta = conta.clone();
            cx.observe(sinal, move |_, _| conta.set(conta.get() + 1)).detach();
        }
    });

    fake.with(|s| s.decorrido = 2.0);
    store.update(cx, |s, cx| s.tick(cx));

    assert_eq!((relogio.get(), tela.get(), fila.get()), (1, 0, 0));
}

#[gpui::test]
fn espaco_toca(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Loaded);
    let store = store(cx, &fake);
    cx.update(|cx| {
        actions::bind(cx);
        actions::register(&store, cx);
    });
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));

    window.simulate_keystrokes("space");
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));

    assert!(fake.chamou("play"));
}

#[gpui::test]
fn cmd_o_abre_o_painel_e_acrescenta_o_que_foi_escolhido(cx: &mut TestAppContext) {
    let pasta = pasta_com("cmd-o", &["a.flac"]);
    let fake = FakePlayer::new();
    let store = store(cx, &fake);
    cx.update(|cx| {
        actions::bind(cx);
        actions::register(&store, cx);
    });
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));

    window.simulate_keystrokes("cmd-o");
    assert!(cx.did_prompt_for_paths());
    let escolhida = pasta.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![escolhida]));
    cx.run_until_parked();
    store.update(cx, |s, _| assert!(s.wait_idle(PRAZO)));

    assert_eq!(fake.with(|s| s.itens.len()), 1);
    std::fs::remove_dir_all(pasta).unwrap();
}
```

Run: `cd apps/gpui && cargo test -p hog-player-gpui --test views`
Expected: falha de compilação com `unresolved import hog_player_gpui::actions` (e `store`, `views`).

- [ ] **Step 3: Tema e ícones**

`apps/gpui/app/src/theme.rs`:

```rust
//! Tokens de cor, espaço e tipo. As cores seguem as do SwiftUI (`Color.hogAccent` e as cores
//! semânticas do sistema), para a comparação medir a tecnologia e não um visual diferente.

use gpui::{Rgba, WindowAppearance, rgb, rgba};

pub const FONT: &str = ".SystemUIFont";
pub const MONO_FONT: &str = "Menlo";

/// A faixa de cima, sob a barra de título transparente, onde fica o botão Abrir (a toolbar do
/// SwiftUI).
pub const TITLEBAR_HEIGHT: f32 = 40.0;

// O bloco abaixo da capa soma `layout::CONTROLS_HEIGHT` (264): 16 + 64 + 16 + 32 + 16 + 56 +
// 16 + 24 + 8 + 16.
pub const GAP: f32 = 16.0;
pub const TITLE_BLOCK: f32 = 64.0;
pub const CLOCK_HEIGHT: f32 = 32.0;
pub const TRANSPORT_HEIGHT: f32 = 56.0;
pub const VOLUME_HEIGHT: f32 = 24.0;
pub const TECHNICAL_GAP: f32 = 8.0;
pub const TECHNICAL_HEIGHT: f32 = 16.0;

pub const STATUS_HEIGHT: f32 = 40.0;
pub const STATUS_GAP: f32 = 12.0;
pub const ROW_HEIGHT: f32 = 56.0;
pub const THUMB: f32 = 44.0;

#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Rgba,
    pub foreground: Rgba,
    pub secondary: Rgba,
    pub tertiary: Rgba,
    pub fill: Rgba,
    pub panel: Rgba,
    pub veil: Rgba,
    pub accent: Rgba,
    pub accent_soft: Rgba,
    pub on_accent: Rgba,
    pub error: Rgba,
    pub error_soft: Rgba,
    pub warning: Rgba,
}

impl Palette {
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
        }
    }

    fn dark() -> Self {
        Self {
            background: rgb(0x1e1e1e),
            foreground: rgb(0xffffff),
            secondary: rgba(0xffffff8c),
            tertiary: rgba(0xffffff59),
            fill: rgba(0xffffff14),
            panel: rgba(0x2a2a2ab8),
            veil: rgba(0x1e1e1e99),
            accent: rgb(0xfa6112),
            accent_soft: rgba(0xfa61121f),
            on_accent: rgb(0xffffff),
            error: rgb(0xff453a),
            error_soft: rgba(0xff453a17),
            warning: rgb(0xff9f0a),
        }
    }

    fn light() -> Self {
        Self {
            background: rgb(0xececec),
            foreground: rgb(0x000000),
            secondary: rgba(0x0000008c),
            tertiary: rgba(0x00000059),
            fill: rgba(0x00000014),
            panel: rgba(0xf6f6f6b8),
            veil: rgba(0xecececa0),
            accent: rgb(0xfa6112),
            accent_soft: rgba(0xfa61121f),
            on_accent: rgb(0xffffff),
            error: rgb(0xff3b30),
            error_soft: rgba(0xff3b3017),
            warning: rgb(0xff9500),
        }
    }
}
```

`apps/gpui/app/src/views/icons.rs` (ícones próprios, em vez de SF Symbols, desenhados aqui para não depender de licença de terceiros):

```rust
//! Ícones monocromáticos: o `svg()` do GPUI pinta com a cor de texto do elemento.

pub const PLAY: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M7 4.5v15l12.5-7.5z"/></svg>"#;
pub const PAUSE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 4h4v16H6zM14 4h4v16h-4z"/></svg>"#;
pub const BACKWARD: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M11 6v12L2 12zM21 6v12l-9-6z"/></svg>"#;
pub const FORWARD: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 6v12l9-6zM13 6v12l9-6z"/></svg>"#;
pub const SPEAKER: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 9h4l5-4v14l-5-4H3z"/><path d="M15.5 8.5a5 5 0 0 1 0 7M18 6a8.5 8.5 0 0 1 0 12" fill="none" stroke="black" stroke-width="2" stroke-linecap="round"/></svg>"#;
pub const SPEAKER_SLASH: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 9h4l5-4v14l-5-4H3z"/><path d="M16 9l6 6M22 9l-6 6" fill="none" stroke="black" stroke-width="2" stroke-linecap="round"/></svg>"#;
pub const MUSIC_NOTE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M9 17.5a3 3 0 1 1-2-2.8V4l11-2v12.5a3 3 0 1 1-2-2.8V5.4L9 6.7z"/></svg>"#;
pub const WAVEFORM: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 10h2v4H3zM7 6h2v12H7zM11 3h2v18h-2zM15 7h2v10h-2zM19 10h2v4h-2z"/></svg>"#;
pub const WARNING: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 2 1 21h22zM11 9h2v6h-2zm0 8h2v2h-2z"/></svg>"#;
pub const PLUS: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M11 4h2v7h7v2h-7v7h-2v-7H4v-2h7z"/></svg>"#;
pub const LIST: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 5h12v2H3zm0 6h12v2H3zm0 6h8v2H3zM20 12.5a2.5 2.5 0 1 1-1.5-2.3V3h2v9.5z"/></svg>"#;
```

`apps/gpui/app/src/views/tooltip.rs`:

```rust
use gpui::{
    AnyView, App, AppContext, Context, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div, px,
};

use crate::theme::Palette;

pub struct Tooltip {
    text: SharedString,
    palette: Palette,
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .bg(self.palette.panel)
            .text_color(self.palette.foreground)
            .text_size(px(12.0))
            .child(self.text.clone())
    }
}

/// O construtor que o `.tooltip(...)` do GPUI pede: a dica é uma view.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |window, cx| {
        let palette = Palette::for_appearance(window.appearance());
        let text = text.clone();
        cx.new(move |_| Tooltip { text, palette }).into()
    }
}
```

- [ ] **Step 4: O Store**

`apps/gpui/app/src/store.rs`:

```rust
//! A ponte entre o controlador e as views. Guarda o `Controller`, faz o poll, e notifica só o
//! sinal de cada parte do modelo que mudou: é o equivalente do `@Observable` do SwiftUI.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use gpui::{App, AppContext, Context, Entity, Task};
use hog_player_model::controller::{Changes, Config, Controller, Model, Wake};
use hog_player_model::port::Player;

/// Uma entidade vazia por parte do modelo. A parte que muda notifica a sua, e só a view que a
/// observa redesenha.
pub struct Signal;

#[derive(Clone)]
pub struct Signals {
    pub display: Entity<Signal>,
    pub clock: Entity<Signal>,
    pub queue: Entity<Signal>,
    pub entries: Entity<Signal>,
    pub volume: Entity<Signal>,
    pub error: Entity<Signal>,
}

impl Signals {
    fn new(cx: &mut App) -> Self {
        Self {
            display: cx.new(|_| Signal),
            clock: cx.new(|_| Signal),
            queue: cx.new(|_| Signal),
            entries: cx.new(|_| Signal),
            volume: cx.new(|_| Signal),
            error: cx.new(|_| Signal),
        }
    }
}

/// O `Wake` do controlador manda um aviso por este canal; o `Store` o transforma num tique.
pub fn wake_channel() -> (Wake, UnboundedReceiver<()>) {
    let (tx, rx) = unbounded();
    (Arc::new(move || {
        let _ = tx.unbounded_send(());
    }), rx)
}

pub struct Store {
    controller: Controller,
    pub signals: Signals,
    selected: Option<u64>,
    last_current: Option<u64>,
    quit_requested: Arc<AtomicBool>,
    tasks: Vec<Task<()>>,
}

impl Store {
    pub fn new(player: Arc<dyn Player>, config: Config, cx: &mut Context<Self>) -> Self {
        Self {
            controller: Controller::new(player, config),
            signals: Signals::new(cx),
            selected: None,
            last_current: None,
            quit_requested: Arc::new(AtomicBool::new(false)),
            tasks: Vec::new(),
        }
    }

    /// Liga o poll de 10 Hz, como o `Timer` do SwiftUI, e o tique na hora quando uma resposta
    /// chega. O timer do GPUI é um `dispatch_after` sem folga; o do Swift tem 20 ms, e essa
    /// diferença fica registrada no resultado.
    pub fn start(&mut self, mut woken: UnboundedReceiver<()>, cx: &mut Context<Self>) {
        const POLL: Duration = Duration::from_millis(100);
        self.tasks.push(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                if this.update(cx, |store, cx| store.tick(cx)).is_err() {
                    break;
                }
            }
        }));
        self.tasks.push(cx.spawn(async move |this, cx| {
            while woken.next().await.is_some() {
                // Várias respostas juntas viram um tique só.
                while woken.try_recv().is_ok() {}
                if this.update(cx, |store, cx| store.tick(cx)).is_err() {
                    break;
                }
            }
        }));
    }

    pub fn model(&self) -> &Model {
        self.controller.model()
    }

    pub fn tick(&mut self, cx: &mut Context<Self>) {
        let changes = self.controller.tick();
        // A seleção acompanha a faixa atual, como o `onChange(of: currentItemID)` do SwiftUI.
        let current = self.controller.model().queue.current_item_id();
        let follows = current != self.last_current;
        if follows {
            self.last_current = current;
            self.selected = current;
        }
        self.publish(Changes { queue: changes.queue || follows, ..changes }, cx);
        if self.quit_requested.load(Ordering::Acquire) {
            cx.quit();
        }
    }

    fn publish(&self, changes: Changes, cx: &mut App) {
        let parts = [
            (changes.display, &self.signals.display),
            (changes.clock, &self.signals.clock),
            (changes.queue, &self.signals.queue),
            (changes.entries, &self.signals.entries),
            (changes.volume, &self.signals.volume),
            (changes.error, &self.signals.error),
        ];
        for (changed, signal) in parts {
            if changed {
                signal.update(cx, |_, cx| cx.notify());
            }
        }
    }

    fn notify(signal: &Entity<Signal>, cx: &mut App) {
        signal.update(cx, |_, cx| cx.notify());
    }

    pub fn selected(&self) -> Option<u64> {
        self.selected
    }

    pub fn toggle(&mut self) {
        self.controller.toggle();
    }

    pub fn next(&mut self) {
        self.controller.next();
    }

    pub fn previous(&mut self) {
        self.controller.previous();
    }

    pub fn clear(&mut self) {
        self.controller.clear();
    }

    pub fn add(&mut self, paths: Vec<PathBuf>) {
        self.controller.add(paths);
    }

    /// O clique numa linha seleciona e toca, como o `onTapGesture` do SwiftUI.
    pub fn play_row(&mut self, id: u64, cx: &mut Context<Self>) {
        self.selected = Some(id);
        self.controller.select(id);
        Self::notify(&self.signals.queue, cx);
    }

    pub fn select_row(&mut self, id: Option<u64>, cx: &mut Context<Self>) {
        self.selected = id;
        Self::notify(&self.signals.queue, cx);
    }

    /// Setas para cima e para baixo movem a seleção, como numa `List`.
    pub fn move_selection(&mut self, delta: i32, cx: &mut Context<Self>) {
        let items = &self.controller.model().queue.items;
        let Some(last) = items.len().checked_sub(1) else { return };
        let position = self.selected.and_then(|id| items.iter().position(|t| t.id == id));
        let target = match position {
            Some(index) => (index as i64 + i64::from(delta)).clamp(0, last as i64) as usize,
            None if delta > 0 => 0,
            None => last,
        };
        self.selected = Some(items[target].id);
        Self::notify(&self.signals.queue, cx);
    }

    pub fn remove_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected.take() {
            self.controller.remove(id);
            Self::notify(&self.signals.queue, cx);
        }
    }

    /// Publica na hora: o slider não espera o core.
    pub fn apply_volume(&mut self, value: f64, cx: &mut Context<Self>) {
        self.controller.apply_volume(value.clamp(0.0, 1.0));
        Self::notify(&self.signals.volume, cx);
    }

    /// O passo do teclado e do VoiceOver.
    pub fn nudge_volume(&mut self, delta: f64, cx: &mut Context<Self>) {
        let value = self.controller.model().volume.value + delta;
        self.apply_volume(value, cx);
    }

    pub fn set_volume_editing(&mut self, editing: bool) {
        self.controller.set_volume_editing(editing);
    }

    pub fn quit_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.quit_requested)
    }

    /// Para o poll e devolve o device. O erro volta para quem chamou registrar em log: a janela
    /// já está fechando.
    pub fn shutdown(&mut self) -> Result<(), String> {
        self.tasks.clear();
        self.controller.shutdown()
    }

    /// Só para testes: espera os comandos em curso sem o poll.
    pub fn wait_idle(&mut self, timeout: Duration) -> bool {
        self.controller.wait_idle(timeout)
    }

    /// Só para testes: espera as linhas da fila carregarem.
    pub fn wait_entries(&mut self, timeout: Duration) -> bool {
        self.controller.wait_entries(timeout)
    }
}
```

- [ ] **Step 5: Ações, atalhos e menus**

`apps/gpui/app/src/actions.rs`:

```rust
//! Atalhos e menus. O SwiftUI tem ⌘O como atalho do botão da barra e espaço no botão de tocar;
//! aqui os dois também entram no menu, e a medição 8 registra a diferença.

use gpui::{App, AppContext, Entity, KeyBinding, Menu, MenuItem, PathPromptOptions, actions};

use crate::store::Store;

actions!(
    hog_player,
    [Quit, Hide, Minimize, Open, PlayPause, RemoveSelected, SelectPrevious, SelectNext]
);

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("cmd-o", Open, None),
        KeyBinding::new("space", PlayPause, None),
        // Sem campo de texto na janela, o Delete vale em qualquer lugar. No SwiftUI ele só
        // funciona com a lista em foco: diferença deliberada, registrada na medição 7.
        KeyBinding::new("backspace", RemoveSelected, None),
        KeyBinding::new("delete", RemoveSelected, None),
        KeyBinding::new("up", SelectPrevious, None),
        KeyBinding::new("down", SelectNext, None),
    ]);
}

pub fn register(store: &Entity<Store>, cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &Minimize, cx| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_, window, _| window.minimize_window());
        }
    });
    let s = store.clone();
    cx.on_action(move |_: &PlayPause, cx| s.update(cx, |store, _| store.toggle()));
    let s = store.clone();
    cx.on_action(move |_: &RemoveSelected, cx| s.update(cx, |store, cx| store.remove_selected(cx)));
    let s = store.clone();
    cx.on_action(move |_: &SelectPrevious, cx| s.update(cx, |store, cx| store.move_selection(-1, cx)));
    let s = store.clone();
    cx.on_action(move |_: &SelectNext, cx| s.update(cx, |store, cx| store.move_selection(1, cx)));
    let s = store.clone();
    cx.on_action(move |_: &Open, cx| open_panel(&s, cx));
}

/// O NSOpenPanel do GPUI não filtra por tipo: o filtro fica para o `audio_files` do controlador.
fn open_panel(store: &Entity<Store>, cx: &mut App) {
    let paths = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: true,
        multiple: true,
        prompt: Some("Abrir".into()),
    });
    let store = store.clone();
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = paths.await {
            cx.update(|cx| store.update(cx, |store, _| store.add(paths)));
        }
    })
    .detach();
}

pub fn menus() -> Vec<Menu> {
    vec![
        Menu::new("hog-audio GPUI").items([
            MenuItem::action("Esconder", Hide),
            MenuItem::separator(),
            MenuItem::action("Sair", Quit),
        ]),
        Menu::new("Arquivo").items([MenuItem::action("Abrir…", Open)]),
        Menu::new("Controles").items([
            MenuItem::action("Tocar ou pausar", PlayPause),
            MenuItem::action("Remover da fila", RemoveSelected),
        ]),
        Menu::new("Janela").items([MenuItem::action("Minimizar", Minimize)]),
    ]
}
```

- [ ] **Step 6: As views da coluna da faixa atual**

`apps/gpui/app/src/views/mod.rs`:

```rust
pub mod clock;
pub mod cover;
pub mod icons;
pub mod root;
pub mod status;
pub mod technical;
pub mod tooltip;
pub mod track_info;
pub mod transport;

use gpui::{Context, Entity, Subscription};

use crate::store::{Signal, Signals, Store};

/// Cada view observa só o sinal da parte do modelo que ela lê.
pub(crate) fn observe<V: 'static>(
    store: &Entity<Store>,
    pick: fn(&Signals) -> &Entity<Signal>,
    cx: &mut Context<V>,
) -> Subscription {
    let signal = pick(&store.read(cx).signals).clone();
    cx.observe(&signal, |_, _, cx| cx.notify())
}
```

`apps/gpui/app/src/views/cover.rs` (a capa de verdade entra na Task 8; aqui é o gradiente do SwiftUI sem capa):

```rust
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
    linear_color_stop, linear_gradient, px, rgb, rgba, svg,
};

use super::{icons, observe};
use crate::store::Store;
use crate::theme::Palette;

pub struct CoverView {
    store: Entity<Store>,
    _display: Subscription,
}

impl CoverView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _display = observe(&store, |s| &s.display, cx);
        Self { store, _display }
    }
}

impl Render for CoverView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        div()
            .size_full()
            .rounded(px(12.0))
            .border_1()
            .border_color(rgba(0xffffff1f))
            .bg(linear_gradient(135.0, linear_color_stop(palette.accent, 0.0), linear_color_stop(rgb(0xb82e05), 1.0)))
            .flex()
            .items_center()
            .justify_center()
            .child(svg().data(icons::WAVEFORM).size(px(64.0)).text_color(rgba(0xffffffeb)))
    }
}
```

`apps/gpui/app/src/views/track_info.rs`:

```rust
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Entity, FontWeight, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    div, px,
};

use super::observe;
use crate::store::Store;
use crate::theme::Palette;

pub struct TrackInfoView {
    store: Entity<Store>,
    _display: Subscription,
}

impl TrackInfoView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _display = observe(&store, |s| &s.display, cx);
        Self { store, _display }
    }
}

impl Render for TrackInfoView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let display = self.store.read(cx).model().display.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .overflow_hidden()
            .child(div().text_size(px(22.0)).font_weight(FontWeight::BOLD).truncate().child(display.title))
            .when(!display.artist.is_empty(), |d| {
                d.child(div().text_size(px(17.0)).text_color(palette.secondary).truncate().child(display.artist))
            })
            .when(!display.album.is_empty(), |d| {
                d.child(div().text_size(px(15.0)).text_color(palette.tertiary).truncate().child(display.album))
            })
    }
}
```

`apps/gpui/app/src/views/clock.rs`:

```rust
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div, px,
    relative,
};

use super::observe;
use crate::store::Store;
use crate::theme::Palette;

/// Lê só o relógio: é o único trecho da tela que muda a cada tique enquanto toca.
pub struct ClockView {
    store: Entity<Store>,
    _clock: Subscription,
}

impl ClockView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _clock = observe(&store, |s| &s.clock, cx);
        Self { store, _clock }
    }
}

impl Render for ClockView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let clock = self.store.read(cx).model().clock.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div().w_full().h(px(4.0)).rounded_full().bg(palette.fill).child(
                    div().h_full().rounded_full().bg(palette.accent).w(relative(clock.progress as f32)),
                ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(12.0))
                    .text_color(palette.secondary)
                    .child(clock.elapsed)
                    .child(clock.total),
            )
    }
}
```

`apps/gpui/app/src/views/transport.rs`:

```rust
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Div, Entity, InteractiveElement, IntoElement, ParentElement, Render, Rgba, Stateful,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px, svg,
};

use super::{icons, observe};
use crate::store::Store;
use crate::theme::Palette;

pub struct TransportView {
    store: Entity<Store>,
    _display: Subscription,
}

impl TransportView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _display = observe(&store, |s| &s.display, cx);
        Self { store, _display }
    }
}

fn round_button(id: &'static str, icon: &'static [u8], diameter: f32, bg: Rgba, fg: Rgba, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(diameter))
        .flex_shrink_0()
        .rounded_full()
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .opacity(if enabled { 1.0 } else { 0.32 })
        .child(svg().data(icon).size(px(diameter * 0.4)).text_color(fg))
}

impl Render for TransportView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let display = self.store.read(cx).model().display.clone();
        let can_toggle = display.can_play || display.can_pause;
        let toggle_icon = if display.is_playing { icons::PAUSE } else { icons::PLAY };
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(24.0))
            .child(
                round_button("anterior", icons::BACKWARD, 40.0, palette.fill, palette.foreground, display.can_go_previous)
                    .when(display.can_go_previous, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| this.store.update(cx, |s, _| s.previous())))
                    }),
            )
            .child(
                round_button("tocar", toggle_icon, 56.0, palette.accent, palette.on_accent, can_toggle)
                    .when(can_toggle, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| this.store.update(cx, |s, _| s.toggle())))
                    }),
            )
            .child(
                round_button("proxima", icons::FORWARD, 40.0, palette.fill, palette.foreground, display.can_go_next)
                    .when(display.can_go_next, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| this.store.update(cx, |s, _| s.next())))
                    }),
            )
    }
}
```

`apps/gpui/app/src/views/technical.rs`:

```rust
use gpui::{Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div, px};

use super::observe;
use crate::store::Store;
use crate::theme::{MONO_FONT, Palette};

pub struct TechnicalView {
    store: Entity<Store>,
    _display: Subscription,
}

impl TechnicalView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _display = observe(&store, |s| &s.display, cx);
        Self { store, _display }
    }
}

impl Render for TechnicalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let line = self.store.read(cx).model().display.technical_line.clone();
        div()
            .size_full()
            .font_family(MONO_FONT)
            .text_size(px(11.0))
            .text_color(palette.secondary)
            .truncate()
            .child(line)
    }
}
```

`apps/gpui/app/src/views/status.rs`:

```rust
use gpui::{Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div, px, svg};

use super::{icons, observe};
use crate::store::Store;
use crate::theme::{Palette, STATUS_HEIGHT};

pub struct StatusView {
    store: Entity<Store>,
    _error: Subscription,
}

impl StatusView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _error = observe(&store, |s| &s.error, cx);
        Self { store, _error }
    }
}

impl Render for StatusView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let Some(message) = self.store.read(cx).model().error.clone() else {
            return div();
        };
        div()
            .w_full()
            .h(px(STATUS_HEIGHT))
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(12.0))
            .rounded(px(8.0))
            .bg(palette.error_soft)
            .text_color(palette.error)
            .text_size(px(12.0))
            .child(svg().data(icons::WARNING).size(px(14.0)).flex_shrink_0().text_color(palette.error))
            .child(div().flex_1().overflow_hidden().line_clamp(2).child(message))
    }
}
```

`apps/gpui/app/src/views/root.rs`:

```rust
use gpui::{
    Context, Div, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, StyleRefinement, Styled, Subscription, Window, div, px, svg,
};
use hog_player_model::layout::{COLUMN_SPACING, PADDING, cover_side};

use super::clock::ClockView;
use super::cover::CoverView;
use super::status::StatusView;
use super::technical::TechnicalView;
use super::tooltip::tooltip;
use super::track_info::TrackInfoView;
use super::transport::TransportView;
use super::{icons, observe};
use crate::actions::Open;
use crate::store::Store;
use crate::theme::*;

pub struct RootView {
    store: Entity<Store>,
    cover: Entity<CoverView>,
    track_info: Entity<TrackInfoView>,
    clock: Entity<ClockView>,
    transport: Entity<TransportView>,
    technical: Entity<TechnicalView>,
    status: Entity<StatusView>,
    focus: FocusHandle,
    _error: Subscription,
}

/// Caching exige tamanho definido: a view em cache é posta em `style`, não medida pelo conteúdo.
fn sized(width: f32, height: f32) -> StyleRefinement {
    StyleRefinement::default().w(px(width)).h(px(height))
}

fn spacer(height: f32) -> Div {
    div().h(px(height)).flex_shrink_0()
}

impl RootView {
    pub fn new(store: Entity<Store>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _error = observe(&store, |s| &s.error, cx);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            cover: cx.new(|cx| CoverView::new(store.clone(), cx)),
            track_info: cx.new(|cx| TrackInfoView::new(store.clone(), cx)),
            clock: cx.new(|cx| ClockView::new(store.clone(), cx)),
            transport: cx.new(|cx| TransportView::new(store.clone(), cx)),
            technical: cx.new(|cx| TechnicalView::new(store.clone(), cx)),
            status: cx.new(|cx| StatusView::new(store.clone(), cx)),
            store,
            focus,
            _error,
        }
    }

    fn left_column(&self, cover: f32) -> Div {
        div()
            .w(px(cover))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .child(self.cover.clone().cached(sized(cover, cover)))
            .child(spacer(GAP))
            .child(self.track_info.clone().cached(sized(cover, TITLE_BLOCK)))
            .child(spacer(GAP))
            .child(self.clock.clone().cached(sized(cover, CLOCK_HEIGHT)))
            .child(spacer(GAP))
            .child(self.transport.clone().cached(sized(cover, TRANSPORT_HEIGHT)))
            .child(spacer(GAP))
            .child(self.volume_slot(cover))
            .child(spacer(TECHNICAL_GAP))
            .child(self.technical.clone().cached(sized(cover, TECHNICAL_HEIGHT)))
    }

    /// O volume entra na Task 6.
    fn volume_slot(&self, cover: f32) -> Div {
        div().w(px(cover)).h(px(VOLUME_HEIGHT))
    }

    /// A fila entra na Task 7.
    fn queue_slot(&self, width: f32, height: f32, palette: &Palette) -> Div {
        div().w(px(width)).h(px(height)).rounded(px(12.0)).bg(palette.panel)
    }

    fn title_bar(&self, palette: &Palette) -> Div {
        let hover = palette.fill;
        div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(TITLEBAR_HEIGHT))
            .flex()
            .items_center()
            .justify_end()
            .pr(px(16.0))
            .child(
                div()
                    .id("abrir")
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded(px(6.0))
                    .hover(move |s| s.bg(hover))
                    .text_size(px(13.0))
                    .child(svg().data(icons::PLUS).size(px(14.0)).text_color(palette.foreground))
                    .child("Abrir")
                    .tooltip(tooltip("Abrir arquivos ou pastas"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(Open), cx)),
            )
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let viewport = window.viewport_size();
        let width = f32::from(viewport.width);
        let height = f32::from(viewport.height) - TITLEBAR_HEIGHT;
        let cover = cover_side(width, height);
        let has_error = self.store.read(cx).model().error.is_some();
        let status = if has_error { STATUS_HEIGHT + STATUS_GAP } else { 0.0 };
        let queue_width = width - 2.0 * PADDING - COLUMN_SPACING - cover;
        let queue_height = height - 2.0 * PADDING - status;

        div()
            .id("raiz")
            .key_context("Player")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .font_family(FONT)
            .bg(palette.background)
            .text_color(palette.foreground)
            .child(self.title_bar(&palette))
            .child(
                div()
                    .absolute()
                    .top(px(TITLEBAR_HEIGHT))
                    .left_0()
                    .w(px(width))
                    .h(px(height))
                    .p(px(PADDING))
                    .flex()
                    .flex_row()
                    .gap(px(COLUMN_SPACING))
                    .child(self.left_column(cover))
                    .child(
                        div()
                            .w(px(queue_width))
                            .h_full()
                            .flex()
                            .flex_col()
                            .gap(px(STATUS_GAP))
                            .child(self.status.clone())
                            .child(self.queue_slot(queue_width, queue_height, &palette)),
                    ),
            )
    }
}
```

`apps/gpui/app/src/lib.rs`:

```rust
//! A interface em GPUI. Desenha o modelo do `hog-player-model` e manda intenções; nenhuma regra
//! de reprodução mora aqui.

pub mod actions;
pub mod store;
pub mod theme;
pub mod views;
```

- [ ] **Step 7: O `main.rs` de verdade**

`apps/gpui/app/src/main.rs` (substitui o spike inteiro):

```rust
use std::sync::Arc;

use gpui::{
    App, AppContext, Bounds, Entity, QuitMode, TitlebarOptions, WindowBounds, WindowOptions,
    point, px, size,
};
use hog_audio::api::HogPlayer;
use hog_player_gpui::actions;
use hog_player_gpui::store::{Store, wake_channel};
use hog_player_gpui::theme::TITLEBAR_HEIGHT;
use hog_player_gpui::views::root::RootView;
use hog_player_model::controller::{Config, LogSink};
use hog_player_model::layout::{DEFAULT_WINDOW, MIN_WINDOW};

fn main() {
    // Criado antes de qualquer janela, como o `PlayerViewModel` no AppDelegate: não toma device
    // nenhum até o primeiro play.
    let player = HogPlayer::new();
    gpui_platform::application()
        // Fechar a última janela encerra, como `applicationShouldTerminateAfterLastWindowClosed`.
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx: &mut App| {
            let (wake, woken) = wake_channel();
            let config = Config::production(transition_log(), wake);
            let store = cx.new(|cx| Store::new(player.clone(), config, cx));
            store.update(cx, |store, cx| store.start(woken, cx));
            actions::bind(cx);
            actions::register(&store, cx);
            cx.set_menus(actions::menus());
            open_window(&store, cx);
            cx.activate(true);
            // Um arquivo pela linha de comando, como no SwiftUI: itera rápido sem o painel.
            if let Some(path) = std::env::args().nth(1) {
                store.update(cx, |store, _| store.add(vec![path.into()]));
            }
        });
}

/// O log de transições vai para stderr; a Task 9 acrescenta o arquivo.
fn transition_log() -> LogSink {
    Arc::new(|line: &str| eprintln!("{line}"))
}

fn open_window(store: &Entity<Store>, cx: &mut App) {
    let (width, height) = DEFAULT_WINDOW;
    let (min_width, min_height) = MIN_WINDOW;
    let bounds = Bounds::centered(None, size(px(width), px(height + TITLEBAR_HEIGHT)), cx);
    let store = store.clone();
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("hog-audio GPUI".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(12.0), px(14.0))),
            }),
            window_min_size: Some(size(px(min_width), px(min_height + TITLEBAR_HEIGHT))),
            app_id: Some("local.hogaudio.gpui".to_string()),
            ..Default::default()
        },
        move |window, cx| cx.new(|cx| RootView::new(store, window, cx)),
    )
    .expect("não consegui abrir a janela");
}
```

- [ ] **Step 8: Rodar e ver passar**

Run: `cd apps/gpui && cargo test -p hog-player-gpui --test views`
Expected: 4 testes passando.

- Se `espaco_toca` não disparar a ação sem foco, conferir que o `RootView::new` focou a raiz.
- Se `try_recv` não existir no `futures` resolvido, usar `try_next`, que é o nome antigo, e rodar o clippy para ver se ele está marcado como obsoleto.

- [ ] **Step 9: Sabotagem**

- Em `Store::publish`, trocar `if changed` por `if true`. `o_tique_do_relogio_notifica_so_o_sinal_do_relogio` tem de reprovar com `(1, 1, 1)`. Desfazer.
- Em `actions::bind`, tirar a linha do `space`. `espaco_toca` tem de reprovar. Desfazer.

- [ ] **Step 10: Verificação visual, sem device**

Run: `cd apps/gpui && cargo run --release -p hog-player-gpui`

A janela abre com as duas colunas:
- à esquerda, o gradiente no lugar da capa e "nenhuma faixa carregada";
- o relógio em 0:00 e os botões de transporte apagados;
- o espaço do volume e a linha técnica vazia;
- à direita, o painel da fila;
- no alto, o botão Abrir.

Conferir também:
- os menus "hog-audio GPUI", "Arquivo", "Controles" e "Janela" na barra, com ⌘O ao lado de "Abrir…";
- redimensionar até o mínimo: a capa encolhe e a fila fica com pelo menos 340;
- arrastar a janela pela faixa de cima.

Não abrir arquivo nenhum: tocar toma o device. Fechar com ⌘Q.

- [ ] **Step 11: Gate e ponto de parada**

Run: `make gpui-fmt && make gpui-test && make test`
Expected: tudo verde. Mostrar `git status --short` e screenshots ao usuário. Commit só se ele pedir, com `feat(gpui): montar janela, store e coluna da faixa atual`.

---

### Task 6: Volume (slider adaptado do Pawse)

**Files:**
- Create: `apps/gpui/app/src/views/volume.rs`
- Modify: `apps/gpui/app/src/views/mod.rs` (`pub mod volume;`)
- Modify: `apps/gpui/app/src/views/root.rs` (campo `volume` e troca do `volume_slot`)
- Test: `apps/gpui/app/tests/views.rs`

**Interfaces:**
- Consumes: `Store::{apply_volume, nudge_volume, set_volume_editing, model}` (Task 5).
- Produces: `views::volume::VolumeView::new(Entity<Store>, &mut Context<Self>)`.

- [ ] **Step 1: Testes (vermelho)**

Acrescentar em `apps/gpui/app/tests/views.rs`:

```rust
#[gpui::test]
fn o_slider_publica_na_hora_e_notifica_so_o_volume(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Playing);
    let store = store(cx, &fake);
    store.update(cx, |s, _| assert!(s.wait_idle(PRAZO) && s.wait_entries(PRAZO)));
    store.update(cx, |s, cx| s.tick(cx));
    let volume = Rc::new(Cell::new(0));
    let tela = Rc::new(Cell::new(0));
    cx.update(|cx| {
        let signals = store.read(cx).signals.clone();
        for (sinal, conta) in [(&signals.volume, &volume), (&signals.display, &tela)] {
            let conta = conta.clone();
            cx.observe(sinal, move |_, _| conta.set(conta.get() + 1)).detach();
        }
    });

    store.update(cx, |s, cx| s.apply_volume(0.3, cx));

    assert_eq!(store.read_with(cx, |s, _| s.model().volume.value), 0.3);
    assert_eq!((volume.get(), tela.get()), (1, 0));
    store.update(cx, |s, _| assert!(s.wait_idle(PRAZO)));
    assert_eq!(fake.with(|s| s.volume_aplicado), Some(0.3));
}

#[gpui::test]
fn o_passo_do_volume_nao_passa_dos_limites(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Playing);
    let store = store(cx, &fake);

    store.update(cx, |s, cx| {
        s.apply_volume(0.98, cx);
        s.nudge_volume(0.05, cx);
    });
    assert_eq!(store.read_with(cx, |s, _| s.model().volume.value), 1.0);

    store.update(cx, |s, cx| s.nudge_volume(-2.0, cx));
    assert_eq!(store.read_with(cx, |s, _| s.model().volume.value), 0.0);
    store.update(cx, |s, _| assert!(s.wait_idle(PRAZO)));
}
```

Run: `cd apps/gpui && cargo test -p hog-player-gpui --test views`
Expected: os dois testes novos passam, porque `apply_volume` e `nudge_volume` já existem desde a Task 5.

**Isso não conta como vermelho.** Então fazer a sabotagem antes de seguir: tirar o `.clamp(0.0, 1.0)` de `Store::apply_volume` e ver `o_passo_do_volume_nao_passa_dos_limites` reprovar; tirar o `Self::notify(&self.signals.volume, cx)` e ver `o_slider_publica_na_hora_e_notifica_so_o_volume` reprovar. Desfazer os dois.

- [ ] **Step 2: A view do volume**

`apps/gpui/app/src/views/volume.rs`:

```rust
//! O slider do volume. O arrasto foi adaptado do slider do Pawse
//! (`crates/ui_components/src/slider.rs`, MIT, © 2026 Aleksandr Popov): um payload de drag vazio
//! e um mouse-up global na fase de captura, a técnica do scrollbar do Zed, para soltar fora do
//! trilho também encerrar o arrasto.

use gpui::prelude::FluentBuilder;
use gpui::{
    Bounds, Context, DispatchPhase, DragMoveEvent, Empty, Entity, EntityId, FontWeight,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement,
    Pixels, Point, Render, StatefulInteractiveElement, Styled, Subscription, Window, canvas, div,
    px, relative, svg,
};

use super::tooltip::tooltip;
use super::{icons, observe};
use crate::store::Store;
use crate::theme::Palette;

#[derive(Clone)]
struct DragVolume(EntityId);

impl Render for DragVolume {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub struct VolumeView {
    store: Entity<Store>,
    track: Bounds<Pixels>,
    dragging: bool,
    _volume: Subscription,
}

impl VolumeView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _volume = observe(&store, |s| &s.volume, cx);
        Self { store, track: Bounds::default(), dragging: false, _volume }
    }

    /// Com a mão no slider, o volume mudado por fora não o move (a regra do controlador).
    fn begin(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.dragging = true;
        self.store.update(cx, |s, _| s.set_volume_editing(true));
        self.seek(position, cx);
    }

    fn seek(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let width = self.track.size.width;
        if width <= px(0.0) {
            return;
        }
        let value = ((position.x - self.track.left()) / width).clamp(0.0, 1.0);
        self.store.update(cx, |s, cx| s.apply_volume(f64::from(value), cx));
    }

    fn end(&mut self, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        self.store.update(cx, |s, _| s.set_volume_editing(false));
    }
}

impl Render for VolumeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let volume = self.store.read(cx).model().volume;
        let fraction = volume.value as f32;
        let silent = volume.muted || volume.value < 0.01;
        let entity = cx.entity();
        let id = cx.entity_id();

        div()
            .size_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .id("mudo")
                    .w(px(20.0))
                    .flex()
                    .justify_center()
                    .child(
                        svg()
                            .data(if silent { icons::SPEAKER_SLASH } else { icons::SPEAKER })
                            .size(px(16.0))
                            .text_color(palette.accent),
                    )
                    .when(volume.muted, |d| d.tooltip(tooltip("O device está no mudo"))),
            )
            .child(
                div()
                    .id("volume")
                    .flex_1()
                    .h(px(16.0))
                    .relative()
                    .flex()
                    .items_center()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| this.begin(event.position, cx)),
                    )
                    .on_drag(DragVolume(id), |drag, _, _, cx| cx.new(|_| drag.clone()))
                    .on_drag_move(cx.listener(move |this, event: &DragMoveEvent<DragVolume>, _, cx| {
                        if event.drag(cx).0 == id {
                            this.seek(event.event.position, cx);
                        }
                    }))
                    .child(
                        div().w_full().h(px(4.0)).rounded_full().bg(palette.fill).child(
                            div().h_full().rounded_full().bg(palette.accent).w(relative(fraction)),
                        ),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(2.0))
                            .left(relative(fraction))
                            .ml(px(-6.0))
                            .size(px(12.0))
                            .rounded_full()
                            .bg(palette.foreground),
                    )
                    .child(
                        canvas(
                            {
                                let entity = entity.clone();
                                move |bounds, _, cx| entity.update(cx, |this, _| this.track = bounds)
                            },
                            move |_, _, window, _| {
                                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                                        entity.update(cx, |this, cx| this.end(cx));
                                    }
                                });
                            },
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .child(
                div()
                    .w(px(40.0))
                    .flex()
                    .justify_end()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.secondary)
                    .child(format!("{}%", (volume.value * 100.0) as i64)),
            )
    }
}
```

`apps/gpui/app/src/views/mod.rs`: acrescentar `pub mod volume;` na lista.

`apps/gpui/app/src/views/root.rs`, com Edit:
- importar `use super::volume::VolumeView;`;
- acrescentar o campo `volume: Entity<VolumeView>` e, no `new`, `volume: cx.new(|cx| VolumeView::new(store.clone(), cx)),`;
- trocar o corpo de `volume_slot` por `self.volume.clone().cached(sized(cover, VOLUME_HEIGHT))`, com o retorno passando de `Div` para `impl IntoElement`, e apagar o comentário "O volume entra na Task 6".

- [ ] **Step 3: Rodar**

Run: `cd apps/gpui && cargo test -p hog-player-gpui --test views`
Expected: 6 testes passando.

- [ ] **Step 4: Verificação visual, sem device**

`cargo run --release -p hog-player-gpui`: o slider aparece em 50% ao abrir (o valor inicial do SwiftUI).
- Arrastar o slider move o preenchimento e o número acompanha.
- Soltar fora do trilho encerra o arrasto: mexer o mouse depois não move o slider.

Sem device tomado, o `set_volume` do core só guarda o pedido para a próxima aquisição (`rust/src/engine.rs:764-778`). Não aparece aviso e nada chega ao hardware, então arrastar aqui é seguro.

- [ ] **Step 5: Gate e ponto de parada**

Run: `make gpui-fmt && make gpui-test && make test`
Expected: tudo verde. Commit só se o usuário pedir, com `feat(gpui): acrescentar slider de volume`.

---

### Task 7: Fila (uniform_list, miniaturas, seleção, Delete)

**Files:**
- Create: `apps/gpui/app/src/images.rs`
- Create: `apps/gpui/app/src/views/queue.rs`
- Modify: `apps/gpui/app/src/lib.rs` (`pub mod images;`), `apps/gpui/app/src/views/mod.rs` (`pub mod queue;`), `apps/gpui/app/src/views/root.rs`
- Test: `apps/gpui/app/tests/views.rs`, `apps/gpui/app/tests/images.rs`

**Interfaces:**
- Consumes:
  - `Model::{queue, entries}` e `QueueEntryInfo::subtitle` (Task 4);
  - `Store::{play_row, select_row, move_selection, remove_selected, selected, clear}` (Task 5);
  - `media::{Raster, Thumbnail, ArtworkRef, Artwork}` (Task 1).
- Produces:
  - `images::render_image(&Raster) -> Option<Arc<RenderImage>>`;
  - `images::ThumbnailCache`: `get(&str, &Thumbnail, &mut App)`, `retain(impl Fn(&str) -> bool, &mut App)`, `len()`;
  - `images::ArtworkSlot::update(Option<&ArtworkRef>, impl Fn(&Artwork) -> Option<&Raster>) -> (Option<Arc<RenderImage>>, Option<Arc<RenderImage>>)`, usado na Task 8;
  - `views::queue::QueueView::new(Entity<Store>, &mut Context<Self>)`.

- [ ] **Step 1: Testes (vermelho)**

`apps/gpui/app/tests/images.rs`:

```rust
use std::sync::Arc;

use gpui::TestAppContext;
use hog_player_gpui::images::{ArtworkSlot, ThumbnailCache, render_image};
use hog_player_model::media::{Artwork, ArtworkRef, Raster, Thumbnail};

fn raster(width: u32, height: u32) -> Raster {
    Raster { width, height, bgra: vec![0, 0, 255, 255].repeat((width * height) as usize) }
}

fn arte() -> ArtworkRef {
    ArtworkRef(Arc::new(Artwork { cover: raster(4, 4), backdrop: Some(raster(2, 2)) }))
}

#[test]
fn raster_invalido_nao_vira_imagem() {
    let quebrado = Raster { width: 4, height: 4, bgra: vec![0; 3] };
    assert!(render_image(&quebrado).is_none());
    assert!(render_image(&raster(2, 1)).is_some());
}

#[test]
fn a_mesma_capa_mostra_a_mesma_imagem_e_nada_sai() {
    let mut slot = ArtworkSlot::default();
    let capa = arte();
    let (primeira, saiu) = slot.update(Some(&capa), |a| Some(&a.cover));
    assert!(primeira.is_some() && saiu.is_none());

    let (segunda, saiu) = slot.update(Some(&capa), |a| Some(&a.cover));
    assert!(Arc::ptr_eq(primeira.as_ref().unwrap(), segunda.as_ref().unwrap()));
    assert!(saiu.is_none());
}

#[test]
fn capa_nova_troca_a_imagem_e_a_velha_sai() {
    let mut slot = ArtworkSlot::default();
    let (velha, _) = slot.update(Some(&arte()), |a| Some(&a.cover));
    let (nova, saiu) = slot.update(Some(&arte()), |a| Some(&a.cover));

    assert!(!Arc::ptr_eq(velha.as_ref().unwrap(), nova.as_ref().unwrap()));
    assert!(Arc::ptr_eq(velha.as_ref().unwrap(), saiu.as_ref().unwrap()));

    let (nenhuma, saiu) = slot.update(None, |a| Some(&a.cover));
    assert!(nenhuma.is_none());
    assert!(Arc::ptr_eq(nova.as_ref().unwrap(), saiu.as_ref().unwrap()));
}

#[gpui::test]
fn a_miniatura_e_convertida_uma_vez_e_sai_quando_a_faixa_sai(cx: &mut TestAppContext) {
    let miniatura = Thumbnail(Arc::new(raster(2, 2)));
    cx.update(|cx| {
        let mut cache = ThumbnailCache::default();
        let a = cache.get("/tmp/a.flac", &miniatura, cx).expect("imagem");
        let b = cache.get("/tmp/a.flac", &miniatura, cx).expect("imagem");
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(cache.len(), 1);

        cache.retain(|path| path != "/tmp/a.flac", cx);
        assert_eq!(cache.len(), 0);
    });
}
```

Acrescentar em `apps/gpui/app/tests/views.rs`:

```rust
#[gpui::test]
fn clicar_numa_linha_seleciona_e_toca(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Loaded);
    let store = store(cx, &fake);

    store.update(cx, |s, cx| s.play_row(2, cx));
    store.update(cx, |s, _| assert!(s.wait_idle(PRAZO)));

    assert_eq!(store.read_with(cx, |s, _| s.selected()), Some(2));
    assert!(fake.chamou("select"));
    assert_eq!(fake.with(|s| s.indice_atual), Some(1));
}

#[gpui::test]
fn a_selecao_acompanha_a_faixa_atual(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Playing);
    let store = store(cx, &fake);
    store.update(cx, |s, cx| s.tick(cx));
    assert_eq!(store.read_with(cx, |s, _| s.selected()), Some(1));

    fake.with(|s| s.indice_atual = Some(2));
    store.update(cx, |s, cx| s.tick(cx));
    assert_eq!(store.read_with(cx, |s, _| s.selected()), Some(3));
}

#[gpui::test]
fn setas_movem_a_selecao_e_delete_remove_a_selecionada(cx: &mut TestAppContext) {
    let fake = carregada(PlayerState::Loaded);
    let store = store(cx, &fake);
    cx.update(|cx| {
        actions::bind(cx);
        actions::register(&store, cx);
    });
    let (_root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));
    store.update(window, |s, cx| s.tick(cx));

    window.simulate_keystrokes("down down up");
    assert_eq!(store.read_with(window, |s, _| s.selected()), Some(2));

    window.simulate_keystrokes("backspace");
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));

    let ids: Vec<u64> = fake.with(|s| s.itens.iter().map(|i| i.id).collect());
    assert_eq!(ids, [1, 3]);
    assert_eq!(store.read_with(window, |s, _| s.selected()), None);
}
```

Run: `cd apps/gpui && cargo test -p hog-player-gpui --test images --test views`
Expected: falha de compilação em `images.rs` (`unresolved import hog_player_gpui::images`). Os três testes novos de `views.rs` dependem só do `Store` da Task 5 e podem passar já. Nesse caso, sabotar `Store::move_selection` (trocar `+ i64::from(delta)` por `- i64::from(delta)`) e `Store::tick` (tirar o `self.selected = current`) para vê-los reprovar, e desfazer.

- [ ] **Step 2: A conversão de imagens**

`apps/gpui/app/src/images.rs`:

```rust
//! `Raster` (BGRA, do modelo) para `RenderImage` (GPU). A conversão só copia bytes: decodificar
//! e reduzir já aconteceu fora da main. As imagens que saem de uso saem do atlas da GPU.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{App, RenderImage};
use hog_player_model::media::{Artwork, ArtworkRef, Raster, Thumbnail};

pub fn render_image(raster: &Raster) -> Option<Arc<RenderImage>> {
    let buffer = image::RgbaImage::from_raw(raster.width, raster.height, raster.bgra.clone())?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// Uma miniatura por caminho da fila, como as linhas do modelo.
#[derive(Default)]
pub struct ThumbnailCache {
    images: HashMap<String, (Thumbnail, Arc<RenderImage>)>,
}

impl ThumbnailCache {
    pub fn get(&mut self, path: &str, thumbnail: &Thumbnail, cx: &mut App) -> Option<Arc<RenderImage>> {
        if let Some((cached, image)) = self.images.get(path)
            && cached == thumbnail
        {
            return Some(Arc::clone(image));
        }
        let image = render_image(&thumbnail.0)?;
        if let Some((_, old)) = self.images.insert(path.to_string(), (thumbnail.clone(), Arc::clone(&image))) {
            cx.drop_image(old, None);
        }
        Some(image)
    }

    pub fn retain(&mut self, keep: impl Fn(&str) -> bool, cx: &mut App) {
        let gone: Vec<String> = self.images.keys().filter(|path| !keep(path)).cloned().collect();
        for path in gone {
            if let Some((_, image)) = self.images.remove(&path) {
                cx.drop_image(image, None);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

/// A imagem de uma capa (a grande ou o fundo) da faixa atual, convertida uma vez por capa.
#[derive(Default)]
pub struct ArtworkSlot {
    shown: Option<(ArtworkRef, Arc<RenderImage>)>,
}

impl ArtworkSlot {
    /// Devolve a imagem a mostrar e a que saiu de uso, para a view tirar do atlas.
    pub fn update(
        &mut self,
        next: Option<&ArtworkRef>,
        pick: impl Fn(&Artwork) -> Option<&Raster>,
    ) -> (Option<Arc<RenderImage>>, Option<Arc<RenderImage>>) {
        if let (Some(next), Some((shown, image))) = (next, &self.shown)
            && next == shown
        {
            return (Some(Arc::clone(image)), None);
        }
        let old = self.shown.take().map(|(_, image)| image);
        self.shown = next.and_then(|art| {
            pick(&art.0).and_then(render_image).map(|image| (art.clone(), image))
        });
        (self.shown.as_ref().map(|(_, image)| Arc::clone(image)), old)
    }
}
```

`apps/gpui/app/src/lib.rs`: acrescentar `pub mod images;`.

- [ ] **Step 3: A view da fila**

`apps/gpui/app/src/views/queue.rs`:

```rust
use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, Render, RenderImage, ScrollStrategy, StatefulInteractiveElement, Styled,
    StyledImage, Subscription, UniformListScrollHandle, Window, div, img, px, rgba, svg,
    uniform_list,
};
use hog_player_model::entry::QueueEntryInfo;
use hog_player_model::media::tags::file_stem;

use super::tooltip::tooltip;
use super::{icons, observe};
use crate::images::ThumbnailCache;
use crate::store::Store;
use crate::theme::{Palette, ROW_HEIGHT, THUMB};

/// O que uma linha visível precisa, copiado do modelo antes de desenhar: a conversão das
/// miniaturas precisa de `&mut App`, que não convive com o empréstimo do `Store`.
struct Row {
    index: usize,
    total: usize,
    id: u64,
    path: String,
    failure: Option<String>,
    entry: Option<QueueEntryInfo>,
    is_current: bool,
    is_playing: bool,
    selected: bool,
}

pub struct QueueView {
    store: Entity<Store>,
    scroll: UniformListScrollHandle,
    thumbnails: ThumbnailCache,
    followed_current: Option<u64>,
    followed_selection: Option<u64>,
    prune: bool,
    _queue: Subscription,
    _entries: Subscription,
}

impl QueueView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _queue = observe(&store, |s| &s.queue, cx);
        let entries = store.read(cx).signals.entries.clone();
        let _entries = cx.observe(&entries, |this, _, cx| {
            this.prune = true;
            cx.notify();
        });
        Self {
            store,
            scroll: UniformListScrollHandle::new(),
            thumbnails: ThumbnailCache::default(),
            followed_current: None,
            followed_selection: None,
            prune: false,
            _queue,
            _entries,
        }
    }

    /// Rola até a faixa atual quando ela muda, e até a seleção quando as setas a movem.
    fn follow(&mut self, cx: &Context<Self>) {
        let store = self.store.read(cx);
        let queue = &store.model().queue;
        let current = queue.current_item_id();
        let selected = store.selected();
        let target = if current != self.followed_current {
            self.followed_current = current;
            current
        } else if selected != self.followed_selection {
            selected
        } else {
            None
        };
        self.followed_selection = selected;
        if let Some(index) = target.and_then(|id| queue.items.iter().position(|t| t.id == id)) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        }
    }

    fn rows(&mut self, range: Range<usize>, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let palette = Palette::for_appearance(window.appearance());
        let rows: Vec<Row> = {
            let store = self.store.read(cx);
            let model = store.model();
            let queue = &model.queue;
            range
                .filter_map(|index| {
                    queue.items.get(index).map(|track| Row {
                        index,
                        total: queue.items.len(),
                        id: track.id,
                        path: track.path.clone(),
                        failure: track.failure.clone(),
                        entry: model.entries.get(&track.path).cloned(),
                        is_current: queue.current_index == Some(index as u32),
                        is_playing: queue.is_playing,
                        selected: store.selected() == Some(track.id),
                    })
                })
                .collect()
        };
        rows.into_iter()
            .map(|row| {
                let thumbnail = row
                    .entry
                    .as_ref()
                    .and_then(|entry| entry.thumbnail.as_ref())
                    .and_then(|thumbnail| self.thumbnails.get(&row.path, thumbnail, cx));
                self.row(row, thumbnail, &palette, cx)
            })
            .collect()
    }

    fn row(&self, row: Row, thumbnail: Option<Arc<RenderImage>>, palette: &Palette, cx: &Context<Self>) -> AnyElement {
        let problem = row.failure.clone().or_else(|| row.entry.as_ref().and_then(|e| e.problem.clone()));
        let title = row.entry.as_ref().map_or_else(|| file_stem(&row.path), |e| e.title.clone());
        let subtitle = row.entry.as_ref().map(QueueEntryInfo::subtitle).unwrap_or_default();
        let (hover, selected_bg, id) = (palette.fill, palette.accent_soft, row.id);
        div()
            .id(("faixa", row.id))
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(8.0))
            .rounded(px(8.0))
            .when(row.selected, |d| d.bg(selected_bg))
            .hover(move |s| s.bg(hover))
            .opacity(if problem.is_some() { 0.5 } else { 1.0 })
            .child(thumb_cell(thumbnail, row.is_current, row.is_playing, palette))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .when(row.is_current, |d| d.font_weight(FontWeight::SEMIBOLD))
                            .truncate()
                            .child(title),
                    )
                    .when(!subtitle.is_empty(), |d| {
                        d.child(div().text_size(px(13.0)).text_color(palette.secondary).truncate().child(subtitle))
                    }),
            )
            .child(trailing(&row, problem.is_some(), palette))
            .when_some(problem, |d, problem| d.tooltip(tooltip(problem)))
            .on_click(cx.listener(move |this, _, _, cx| this.store.update(cx, |s, cx| s.play_row(id, cx))))
            .into_any_element()
    }
}

fn thumb_cell(image: Option<Arc<RenderImage>>, is_current: bool, is_playing: bool, palette: &Palette) -> Div {
    div()
        .size(px(THUMB))
        .flex_shrink_0()
        .relative()
        .rounded(px(6.0))
        .overflow_hidden()
        .bg(palette.fill)
        .flex()
        .items_center()
        .justify_center()
        .map(|d| match image {
            Some(image) => d.child(img(image).size_full().rounded(px(6.0)).object_fit(ObjectFit::Cover)),
            None => d.child(svg().data(icons::MUSIC_NOTE).size(px(18.0)).text_color(palette.secondary)),
        })
        .when(is_current, |d| {
            d.child(
                div()
                    .absolute()
                    .size_full()
                    .bg(rgba(0x00000073))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .data(if is_playing { icons::WAVEFORM } else { icons::SPEAKER })
                            .size(px(16.0))
                            .text_color(palette.accent),
                    ),
            )
        })
}

fn trailing(row: &Row, has_problem: bool, palette: &Palette) -> Div {
    let mut trailing = div().flex().items_center().gap(px(8.0)).flex_shrink_0();
    if has_problem {
        trailing = trailing.child(svg().data(icons::WARNING).size(px(16.0)).text_color(palette.warning));
    } else if let Some(entry) = &row.entry
        && let Some(quality) = &entry.quality
    {
        trailing = trailing.child(
            div()
                .id(("selo", row.id))
                .px(px(8.0))
                .py(px(2.0))
                .rounded_full()
                .bg(palette.fill)
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.secondary)
                .child(quality.clone())
                .when_some(entry.quality_detail.clone(), |badge, detail| badge.tooltip(tooltip(detail))),
        );
    }
    if let Some(duration) = row.entry.as_ref().and_then(|e| e.duration.clone()) {
        trailing = trailing.child(
            div()
                .min_w(px(36.0))
                .flex()
                .justify_end()
                .text_size(px(12.0))
                .text_color(palette.secondary)
                .child(duration),
        );
    }
    trailing
}

impl Render for QueueView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        if std::mem::take(&mut self.prune) {
            let keep: HashSet<String> = self.store.read(cx).model().entries.keys().cloned().collect();
            self.thumbnails.retain(|path| keep.contains(path), cx);
        }
        self.follow(cx);
        let count = self.store.read(cx).model().queue.items.len();
        let empty = count == 0;
        let clear_color = if empty { palette.secondary } else { palette.accent };

        let header = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(4.0))
            .child(div().text_size(px(11.0)).font_weight(FontWeight::BOLD).text_color(palette.secondary).child("FILA"))
            .child(
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded_full()
                    .bg(palette.accent_soft)
                    .text_size(px(10.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(palette.accent)
                    .child(count.to_string()),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("limpar")
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(clear_color)
                    .child("Limpar")
                    .when(!empty, |b| b.on_click(cx.listener(|this, _, _, cx| this.store.update(cx, |s, _| s.clear())))),
            );

        let body = if empty {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(svg().data(icons::LIST).size(px(24.0)).text_color(palette.accent))
                .child(div().text_size(px(13.0)).text_color(palette.secondary).child("Arraste músicas ou uma pasta para começar"))
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h(px(0.0))
                .child(
                    uniform_list("fila", count, cx.processor(|this, range: Range<usize>, window, cx| this.rows(range, window, cx)))
                        .track_scroll(&self.scroll)
                        .size_full(),
                )
                .into_any_element()
        };

        div()
            .size_full()
            .p(px(12.0))
            .rounded(px(12.0))
            .bg(palette.panel)
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(header)
            .child(body)
    }
}
```

`apps/gpui/app/src/views/mod.rs`: acrescentar `pub mod queue;`.

`apps/gpui/app/src/views/root.rs`, com Edit:
- importar `use super::queue::QueueView;`;
- acrescentar o campo `queue: Entity<QueueView>` e, no `new`, `queue: cx.new(|cx| QueueView::new(store.clone(), cx)),`;
- trocar `queue_slot` por `self.queue.clone().cached(sized(width, height))`. A assinatura passa a ser `fn queue_slot(&self, width: f32, height: f32) -> impl IntoElement` e a chamada no `render` perde o `&palette`;
- apagar o comentário "A fila entra na Task 7".

- [ ] **Step 4: Rodar**

Run: `cd apps/gpui && cargo test -p hog-player-gpui --test images --test views`
Expected: 4 testes em `images.rs` e 9 em `views.rs` passando.

- [ ] **Step 5: Sabotagem**

- Em `ThumbnailCache::get`, tirar o `if let Some((cached, image)) ... return`. `a_miniatura_e_convertida_uma_vez_e_sai_quando_a_faixa_sai` tem de reprovar no `ptr_eq`. Desfazer.
- Em `ArtworkSlot::update`, trocar o retorno do segundo valor por `None`. `capa_nova_troca_a_imagem_e_a_velha_sai` tem de reprovar. Desfazer.

- [ ] **Step 6: Verificação visual, sem device**

`cargo run --release -p hog-player-gpui -- testdata/meta_fixture.flac` (caminho relativo à raiz do repo, então rodar da raiz: `apps/gpui/target/release/hog-player-gpui testdata/meta_fixture.flac`).

**Carregar uma faixa não toma o device**: o `load` negocia, mas o hog só é tomado no play. Não apertar Tocar.

A fila mostra a linha com miniatura laranja, "Titulo Teste", "Artista Teste — Album Teste", selo `16/44.1` com tooltip e duração `0:02`. Conferir também:
- ⌘O com a pasta `testdata/` inteira: a fila enche aos poucos, de cima para baixo;
- as setas movem a seleção e a lista rola junto;
- Delete remove a linha selecionada;
- Limpar esvazia a fila.

Fechar com ⌘Q.

- [ ] **Step 7: Gate e ponto de parada**

Run: `make gpui-fmt && make gpui-test && make test`
Expected: tudo verde. Commit só se o usuário pedir, com `feat(gpui): acrescentar fila com miniaturas, selecao e Delete`.

---

### Task 8: Capa, fundo com cruzamento e arrastar do Finder

**Files:**
- Modify: `apps/gpui/app/src/views/cover.rs`
- Create: `apps/gpui/app/src/views/backdrop.rs`
- Modify: `apps/gpui/app/src/views/mod.rs` (`pub mod backdrop;`), `apps/gpui/app/src/views/root.rs`
- Test: `apps/gpui/app/tests/views.rs`

**Interfaces:**
- Consumes: `images::{ArtworkSlot, render_image}` (Task 7), `DisplayState::artwork` (Task 1).
- Produces:
  - `views::backdrop::BackdropView::new(Entity<Store>, &mut Context<Self>)`;
  - `RootView::accept_drop(&mut self, Vec<PathBuf>, &mut Context<Self>)`.

- [ ] **Step 1: Teste (vermelho)**

Acrescentar em `apps/gpui/app/tests/views.rs`:

```rust
#[gpui::test]
fn soltar_arquivos_na_janela_acrescenta_a_fila(cx: &mut TestAppContext) {
    let pasta = pasta_com("soltar-janela", &["a.flac", "b.flac"]);
    let fake = FakePlayer::new();
    let store = store(cx, &fake);
    let (root, window) = cx.add_window_view(|window, cx| RootView::new(store.clone(), window, cx));

    root.update(window, |root, cx| root.accept_drop(vec![pasta.clone()], cx));
    store.update(window, |s, _| assert!(s.wait_idle(PRAZO)));

    assert_eq!(fake.with(|s| s.itens.len()), 2);
    std::fs::remove_dir_all(pasta).unwrap();
}
```

Run: `cd apps/gpui && cargo test -p hog-player-gpui --test views soltar_arquivos_na_janela`
Expected: falha de compilação com `no method named accept_drop`.

- [ ] **Step 2: A capa de verdade**

`apps/gpui/app/src/views/cover.rs` (substitui o arquivo):

```rust
use std::sync::Arc;

use gpui::{
    Context, Div, Entity, IntoElement, ObjectFit, ParentElement, Render, RenderImage, Styled,
    StyledImage, Subscription, Window, div, img, linear_color_stop, linear_gradient, px, rgb,
    rgba, svg,
};

use super::{icons, observe};
use crate::images::ArtworkSlot;
use crate::store::Store;
use crate::theme::Palette;

/// A capa da faixa atual, convertida uma vez por capa. Até a nova chegar, o controlador mantém a
/// da faixa anterior na tela, em vez de piscar o placeholder.
pub struct CoverView {
    store: Entity<Store>,
    slot: ArtworkSlot,
    _display: Subscription,
}

impl CoverView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _display = observe(&store, |s| &s.display, cx);
        Self { store, slot: ArtworkSlot::default(), _display }
    }
}

fn placeholder(palette: &Palette) -> Div {
    div()
        .size_full()
        .bg(linear_gradient(135.0, linear_color_stop(palette.accent, 0.0), linear_color_stop(rgb(0xb82e05), 1.0)))
        .flex()
        .items_center()
        .justify_center()
        .child(svg().data(icons::WAVEFORM).size(px(64.0)).text_color(rgba(0xffffffeb)))
}

impl Render for CoverView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let artwork = self.store.read(cx).model().display.artwork.clone();
        let (image, gone): (Option<Arc<RenderImage>>, _) =
            self.slot.update(artwork.as_ref(), |art| Some(&art.cover));
        if let Some(gone) = gone {
            let _ = window.drop_image(gone);
        }
        let content = match image {
            Some(image) => img(image).size_full().rounded(px(12.0)).object_fit(ObjectFit::Cover).into_any_element(),
            None => placeholder(&palette).into_any_element(),
        };
        div()
            .size_full()
            .rounded(px(12.0))
            .overflow_hidden()
            .border_1()
            .border_color(rgba(0xffffff1f))
            .shadow_lg()
            .child(content)
    }
}
```

- [ ] **Step 3: O fundo desfocado com cruzamento**

`apps/gpui/app/src/views/backdrop.rs`:

```rust
//! O fundo da janela: a capa da faixa atual já desfocada, sob um véu da cor da janela que mantém
//! o texto legível. Na troca de faixa, as duas se cruzam em 400 ms, o `easeInOut(0.4)` do
//! SwiftUI. A imagem vem pronta do controlador; a técnica de desfocar na CPU vem do Pawse.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Entity, IntoElement, ObjectFit, ParentElement, Render, RenderImage, Styled,
    StyledImage, Subscription, Window, div, ease_in_out, img, linear_color_stop, linear_gradient,
};

use super::observe;
use crate::images::ArtworkSlot;
use crate::store::Store;
use crate::theme::Palette;

const FADE: Duration = Duration::from_millis(400);

pub struct BackdropView {
    store: Entity<Store>,
    slot: ArtworkSlot,
    current: Option<Arc<RenderImage>>,
    previous: Option<Arc<RenderImage>>,
    fade_started: Option<Instant>,
    _display: Subscription,
}

impl BackdropView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let _display = observe(&store, |s| &s.display, cx);
        Self {
            store,
            slot: ArtworkSlot::default(),
            current: None,
            previous: None,
            fade_started: None,
            _display,
        }
    }
}

impl Render for BackdropView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let artwork = self.store.read(cx).model().display.artwork.clone();
        let (image, gone) = self.slot.update(artwork.as_ref(), |art| art.backdrop.as_ref());
        if gone.is_some() || (image.is_some() && self.current.is_none()) {
            if let Some(stale) = self.previous.take() {
                let _ = window.drop_image(stale);
            }
            self.previous = gone;
            self.current = image;
            self.fade_started = Some(Instant::now());
        }

        let progress = self.fade_started.map_or(1.0, |started| {
            ease_in_out((started.elapsed().as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0))
        });
        if progress < 1.0 {
            // Anima só durante o cruzamento; parado, o fundo não pede quadro nenhum.
            window.request_animation_frame();
        } else {
            self.fade_started = None;
            if let Some(stale) = self.previous.take() {
                let _ = window.drop_image(stale);
            }
        }

        let has_image = self.current.is_some() || self.previous.is_some();
        div()
            .size_full()
            .relative()
            .bg(linear_gradient(180.0, linear_color_stop(palette.background, 0.0), linear_color_stop(palette.accent_soft, 1.0)))
            .when_some(self.previous.clone(), |d, image| {
                d.child(img(image).absolute().size_full().object_fit(ObjectFit::Cover).opacity(1.0 - progress))
            })
            .when_some(self.current.clone(), |d, image| {
                d.child(img(image).absolute().size_full().object_fit(ObjectFit::Cover).opacity(progress))
            })
            .when(has_image, |d| d.child(div().absolute().size_full().bg(palette.veil)))
    }
}
```

`apps/gpui/app/src/views/mod.rs`: acrescentar `pub mod backdrop;`.

`apps/gpui/app/src/views/root.rs`, com Edit:
- importar `use std::path::PathBuf;`, `use gpui::{ExternalPaths, rgba};` (juntando aos imports de `gpui`) e `use super::backdrop::BackdropView;`;
- acrescentar o campo `backdrop: Entity<BackdropView>` e, no `new`, `backdrop: cx.new(|cx| BackdropView::new(store.clone(), cx)),`. O destaque durante o arrasto é o `drag_over` do GPUI, sem estado próprio;
- acrescentar o método:

```rust
    /// Soltar do Finder: o controlador expande pastas e filtra o que não é áudio.
    pub fn accept_drop(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.store.update(cx, |s, _| s.add(paths));
        cx.notify();
    }
```

- no `render`, logo depois de `.text_color(palette.foreground)`, acrescentar o fundo como primeiro filho e o alvo do drop:

```rust
            .child(self.backdrop.clone().cached(StyleRefinement::default().absolute().size_full()))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.accept_drop(paths.paths().to_vec(), cx);
            }))
            .drag_over::<ExternalPaths>(|style, _, _, _| style.border_3().border_color(rgba(0xfa6112e6)))
```

- [ ] **Step 4: Rodar**

Run: `cd apps/gpui && cargo test -p hog-player-gpui`
Expected: tudo verde, com `soltar_arquivos_na_janela_acrescenta_a_fila` passando.

- [ ] **Step 5: Sabotagem**

Trocar o corpo de `accept_drop` por `cx.notify();`. `soltar_arquivos_na_janela_acrescenta_a_fila` tem de reprovar. Desfazer.

- [ ] **Step 6: Verificação visual, sem device**

`apps/gpui/target/release/hog-player-gpui testdata/meta_fixture.flac` (da raiz do repo):
- a capa laranja aparece grande, com borda e sombra;
- o fundo fica laranja desfocado, sob o véu.

Arrastar do Finder `testdata/meta_fixture.m4a` e `testdata/meta_fixture.mp3` juntos:
- a janela mostra a borda laranja durante o arrasto;
- as duas faixas entram na fila.

Clicar numa linha sem tocar não é possível (o clique toca e toma o device). **Não clicar**: a troca de capa com cruzamento é verificada na Task 11, com o device.

- [ ] **Step 7: Gate e ponto de parada**

Run: `make gpui-fmt && make gpui-test && make test`
Expected: tudo verde. Commit só se o usuário pedir, com `feat(gpui): acrescentar capa, fundo desfocado e arrastar do Finder`.

---

### Task 9: Saída (⌘Q, sinais, pânico e log em arquivo)

**Files:**
- Create: `apps/gpui/app/src/exit.rs`
- Modify: `apps/gpui/app/src/lib.rs` (`pub mod exit;`), `apps/gpui/app/src/main.rs`, `apps/gpui/app/src/actions.rs`, `apps/gpui/app/Cargo.toml` (feature `panic-probe`)
- Modify: `apps/gpui/testing/src/lib.rs` (portão no `shutdown`)
- Test: `apps/gpui/app/tests/exit.rs` (binário próprio: o panic hook é global ao processo)

**Interfaces:**
- Consumes: `Store::{shutdown, quit_flag}` (Task 5), `port::Player` (Task 1).
- Produces: `exit::{transition_log(Option<PathBuf>) -> LogSink, log_path() -> Option<PathBuf>, watch_signals(Arc<AtomicBool>) -> io::Result<()>, install_panic_hook(Arc<dyn Player>)}`.

- [ ] **Step 1: Portão no `shutdown` do dublê**

Em `apps/gpui/testing/src/lib.rs`, acrescentar ao `FakeState` o campo `pub portao_do_shutdown: Option<Arc<Gate>>` (com `portao_do_shutdown: None` no `Default`), e trocar o `shutdown` do `impl Player for FakePlayer` por:

```rust
    fn shutdown(&self) -> Result<(), PlayerError> {
        let portao = {
            let mut s = self.lock();
            s.chamadas.push("shutdown".to_string());
            s.portao_do_shutdown.clone()
        };
        if let Some(portao) = portao {
            portao.wait();
        }
        Ok(())
    }
```

- [ ] **Step 2: Testes (vermelho)**

`apps/gpui/app/tests/exit.rs`:

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use hog_player_gpui::exit::{install_panic_hook, transition_log, watch_signals};
use hog_player_testing::{FakePlayer, Gate, PRAZO};

#[test]
fn o_log_de_transicoes_vai_para_o_arquivo() {
    let path = std::env::temp_dir().join(format!("hog-gpui-log-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&path);

    let log = transition_log(Some(path.clone()));
    log("seleção: espera 1 ms, comando 90 ms");

    let conteudo = std::fs::read_to_string(&path).expect("log");
    assert!(conteudo.contains("seleção: espera 1 ms, comando 90 ms"), "{conteudo}");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn sinal_pede_para_sair() {
    let quit = Arc::new(AtomicBool::new(false));
    watch_signals(Arc::clone(&quit)).expect("sinais");

    signal_hook::low_level::raise(signal_hook::consts::SIGHUP).expect("raise");

    let limite = Instant::now() + PRAZO;
    while !quit.load(Ordering::Acquire) && Instant::now() < limite {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(quit.load(Ordering::Acquire));
}

/// Um teste só neste arquivo mexe no panic hook, que é global ao processo. Primeiro o caminho
/// normal (o device é devolvido), depois o engine travado (o hook desiste no teto).
#[test]
fn o_panico_tenta_devolver_o_device_com_teto() {
    let fake = FakePlayer::new();
    install_panic_hook(fake.clone());

    let _ = std::thread::spawn(|| panic!("pânico de teste")).join();
    assert!(fake.chamou("shutdown"));

    let portao = Gate::new();
    fake.with(|s| s.portao_do_shutdown = Some(portao.clone()));
    let antes = Instant::now();
    let _ = std::thread::spawn(|| panic!("pânico com o engine travado")).join();
    assert!(antes.elapsed() < Duration::from_millis(1500), "o hook não respeitou o teto");
    portao.open();
}
```

`signal_hook::low_level::raise` vem do crate `signal-hook`, que já é dependência do app. Run: `cd apps/gpui && cargo test -p hog-player-gpui --test exit`
Expected: falha de compilação com `unresolved import hog_player_gpui::exit`.

- [ ] **Step 3: Implementar**

`apps/gpui/app/src/exit.rs`:

```rust
//! Sair sem deixar o device travado: log de transições, sinais do terminal e pânico. O caminho
//! normal (⌘Q, fechar a janela, sinal) sempre termina no `on_app_quit`, que chama o shutdown.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hog_player_model::controller::LogSink;
use hog_player_model::port::Player;
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook::iterator::Signals;

pub fn log_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Logs/hog-player-gpui.log"))
}

/// Stderr e um arquivo, no mesmo formato de linha do Swift. Rodando pelo .app o stderr se perde,
/// e é do arquivo que a medição 5 lê quando o medidor externo não serve.
pub fn transition_log(path: Option<PathBuf>) -> LogSink {
    let file: Option<Mutex<File>> = path
        .and_then(|path| OpenOptions::new().create(true).append(true).open(path).ok())
        .map(Mutex::new);
    Arc::new(move |line: &str| {
        eprintln!("{line}");
        if let Some(file) = &file {
            let millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_millis());
            let mut file = file.lock().unwrap_or_else(|p| p.into_inner());
            let _ = writeln!(file, "{millis} {line}");
        }
    })
}

/// Ctrl+C, SIGTERM, SIGHUP e SIGQUIT viram um pedido de saída que o próximo tique atende, pelo
/// mesmo caminho do ⌘Q. O handler do signal-hook só marca: nada que não seja seguro em sinal.
pub fn watch_signals(quit: Arc<AtomicBool>) -> std::io::Result<()> {
    let mut signals = Signals::new([SIGINT, SIGTERM, SIGHUP, SIGQUIT])?;
    std::thread::Builder::new()
        .name("hog-signals".to_string())
        .spawn(move || {
            if signals.forever().next().is_some() {
                quit.store(true, Ordering::Release);
            }
        })?;
    Ok(())
}

/// Um pânico nos callbacks do AppKit aborta o processo, porque eles são `extern "C"`. Antes
/// disso, o hook tenta devolver o device, com teto: se o engine estiver travado no próprio
/// pânico, desiste e deixa o macOS soltar o hog. O rate fica trocado (a armadilha conhecida).
pub fn install_panic_hook(player: Arc<dyn Player>) {
    const RESTORE_LIMIT: Duration = Duration::from_millis(500);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        let player = Arc::clone(&player);
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("hog-panic-restore".to_string())
            .spawn(move || {
                let _ = tx.send(player.shutdown());
            });
        if spawned.is_err() {
            eprintln!("pânico: não consegui criar a thread para devolver o device");
            return;
        }
        match rx.recv_timeout(RESTORE_LIMIT) {
            Ok(Ok(())) => eprintln!("pânico: device devolvido"),
            Ok(Err(error)) => eprintln!("pânico: falha ao devolver o device: {error}"),
            Err(_) => eprintln!("pânico: o engine não respondeu em {} ms", RESTORE_LIMIT.as_millis()),
        }
    }));
}
```

`apps/gpui/app/src/lib.rs`: acrescentar `pub mod exit;`.

`apps/gpui/app/Cargo.toml`: acrescentar

```toml
[features]
# Medição 9: um item de menu que provoca pânico num handler de ação.
panic-probe = []
```

`apps/gpui/app/src/actions.rs`, com Edit:
- acrescentar depois do `actions!` existente:

```rust
#[cfg(feature = "panic-probe")]
actions!(hog_player_debug, [ProvokePanic]);
```

- no fim de `register`:

```rust
    #[cfg(feature = "panic-probe")]
    cx.on_action(|_: &ProvokePanic, _| panic!("pânico provocado pela medição 9"));
```

- em `menus`, trocar `vec![...]` por um `let mut menus = vec![...];`, acrescentar

```rust
    #[cfg(feature = "panic-probe")]
    menus.push(Menu::new("Depurar").items([MenuItem::action("Provocar pânico", ProvokePanic)]));
    menus
```

`apps/gpui/app/src/main.rs`, com Edit:
- trocar a função `transition_log` local pelo import `use hog_player_gpui::exit::{install_panic_hook, log_path, transition_log, watch_signals};` e apagar a função;
- trocar `Config::production(transition_log(), wake)` por `Config::production(transition_log(log_path()), wake)`;
- logo antes de `gpui_platform::application()`, acrescentar `install_panic_hook(player.clone());`;
- dentro do `run`, logo depois de `store.update(cx, |store, cx| store.start(woken, cx));`, acrescentar:

```rust
            if let Err(error) = watch_signals(store.read(cx).quit_flag()) {
                eprintln!("não consegui instalar os sinais: {error}");
            }
            // Síncrono de propósito: o future do `on_app_quit` tem teto de 200 ms, o corpo do
            // closure não. Devolver o device não pode ser cortado no meio.
            cx.on_app_quit({
                let store = store.clone();
                let log = transition_log(log_path());
                move |cx| {
                    if let Err(error) = store.update(cx, |store, _| store.shutdown()) {
                        log(&format!("falha ao encerrar o player: {error}"));
                    }
                    std::future::ready(())
                }
            })
            .detach();
```

- [ ] **Step 4: Rodar**

Run: `cd apps/gpui && cargo test -p hog-player-gpui --test exit && cargo build -p hog-player-gpui --features panic-probe`
Expected: 3 testes passando, e o build com a feature compilando.

- [ ] **Step 5: Sabotagem**

- Em `install_panic_hook`, trocar `rx.recv_timeout(RESTORE_LIMIT)` por `rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)`. `o_panico_tenta_devolver_o_device_com_teto` tem de reprovar ou travar. Rodar com `timeout 30 cargo test ...` e desfazer.
- Em `watch_signals`, tirar o `quit.store(true, ...)`. `sinal_pede_para_sair` tem de reprovar. Desfazer.

- [ ] **Step 6: Verificação, sem device**

- `apps/gpui/target/release/hog-player-gpui` e Ctrl+C no terminal: a janela fecha em até 100 ms e o processo termina com código 0.
- ⌘Q: termina.
- Fechar a janela pelo botão vermelho: termina.

Nos três casos, `tail -3 ~/Library/Logs/hog-player-gpui.log` não mostra "falha ao encerrar". Sem device tomado, o shutdown é trivial; com o device, a verificação vem na Task 11.

- [ ] **Step 7: Gate e ponto de parada**

Run: `make gpui-fmt && make gpui-test && make test`
Expected: tudo verde. Commit só se o usuário pedir, com `feat(gpui): devolver o device no quit, em sinal e em panico`.

---

### Task 10: Acessibilidade (AccessKit)

**Exceção declarada ao TDD, para o usuário aprovar na revisão do plano.** O `TestAppContext` não expõe a árvore do AccessKit (relatório, seção 7), então não há teste automático que veja um rótulo. A prova desta tarefa é a travessia da árvore pelo macos-use no Step 2 e a medição 7. Se o usuário não aceitar, a alternativa é um teste que chama `write_a11y_info` por um caminho interno do GPUI, que a tag não expõe como API pública.

**Files:**
- Modify: `apps/gpui/app/src/views/root.rs`, `transport.rs`, `volume.rs`, `queue.rs`, `track_info.rs`, `clock.rs`, `technical.rs`, `status.rs`, `cover.rs`

**Interfaces:**
- Consumes: `gpui::{Role, AccessibleAction}` (reexport do accesskit, relatório seção 7) e `StatefulInteractiveElement::{role, aria_label, aria_selected, aria_position_in_set, aria_size_of_set, aria_numeric_value, aria_min_numeric_value, aria_max_numeric_value, aria_numeric_value_step, on_a11y_action}`.

- [ ] **Step 1: Papéis e rótulos**

Regra do relatório: só `div` com `.id()`, `text!` e `InteractiveText` geram nó. Texto solto não entra na árvore, então cada bloco de texto ganha `.id(...)` e `.aria_label(...)` no contêiner.

- **`transport.rs`.** Depois de cada `round_button(...)`, encadear `.role(Role::Button).aria_label(...)`: "Faixa anterior", `if display.is_playing { "Pausar" } else { "Tocar" }` e "Próxima faixa". O `on_click` já acrescenta a ação Click.
- **`root.rs`.** No botão `abrir`, `.role(Role::Button).aria_label("Abrir arquivos ou pastas")`.
- **`queue.rs`.**
  - O botão `limpar` recebe `.role(Role::Button).aria_label("Limpar a fila")`.
  - O contêiner da lista, o `div().flex_1().min_h(px(0.0))` que envolve o `uniform_list`, recebe `.id("fila-lista").role(Role::List).aria_label("Fila")`.
  - Em `row`, logo depois de `.id(("faixa", row.id))`, acrescentar:

```rust
            .role(Role::ListItem)
            .aria_label(row_label(&row, &title, &subtitle, problem.as_deref()))
            .aria_position_in_set(row.index + 1)
            .aria_size_of_set(row.total)
            .aria_selected(row.selected)
```

  - E a função:

```rust
/// O que o VoiceOver lê numa linha: título, artista e álbum, qualidade, duração e o estado.
fn row_label(row: &Row, title: &str, subtitle: &str, problem: Option<&str>) -> String {
    let entry = row.entry.as_ref();
    let mut parts = vec![title.to_string()];
    parts.extend([Some(subtitle.to_string()).filter(|s| !s.is_empty()), entry.and_then(|e| e.quality.clone()), entry.and_then(|e| e.duration.clone())].into_iter().flatten());
    if row.is_current {
        parts.push(if row.is_playing { "tocando".to_string() } else { "atual".to_string() });
    }
    if let Some(problem) = problem {
        parts.push(format!("problema: {problem}"));
    }
    parts.join(", ")
}
```

- **`volume.rs`.** No `div().id("volume")`, acrescentar:

```rust
                    .role(Role::Slider)
                    .aria_label("Volume")
                    .aria_numeric_value(volume.value * 100.0)
                    .aria_min_numeric_value(0.0)
                    .aria_max_numeric_value(100.0)
                    .aria_numeric_value_step(5.0)
                    .on_a11y_action(AccessibleAction::Increment, {
                        let store = self.store.clone();
                        move |_, _, cx| store.update(cx, |s, cx| s.nudge_volume(0.05, cx))
                    })
                    .on_a11y_action(AccessibleAction::Decrement, {
                        let store = self.store.clone();
                        move |_, _, cx| store.update(cx, |s, cx| s.nudge_volume(-0.05, cx))
                    })
```

  O ícone `mudo`, quando `volume.muted`, recebe `.role(Role::Image).aria_label("O device está no mudo")`.
- **`track_info.rs`.** O `div()` raiz ganha `.id("faixa-atual").role(Role::Label).aria_label(...)`, com o título, o artista e o álbum não vazios juntados por ", ". Calcular o rótulo antes de mover as `String`s para os filhos.
- **`clock.rs`.** O raiz ganha `.id("relogio").role(Role::Label).aria_label(format!("{} de {}", clock.elapsed, clock.total))`.
- **`technical.rs`.** O raiz ganha `.id("linha-tecnica").role(Role::Label).aria_label(line.clone())`, só quando a linha não está vazia (`.when(!line.is_empty(), ...)`).
- **`status.rs`.** O banner ganha `.id("aviso").role(Role::Alert).aria_label(message.clone())`.
- **`cover.rs`.** O raiz ganha `.id("capa").role(Role::Image).aria_label("Capa")`.

Importar `gpui::{Role, AccessibleAction}` e `StatefulInteractiveElement`/`InteractiveElement` onde faltar.

- [ ] **Step 2: Verificação pela árvore, sem device**

Run: `apps/gpui/target/release/hog-player-gpui testdata/meta_fixture.flac` (da raiz, depois de `make gpui`).

Com o macos-use (`open_application_and_traverse` no processo `hog-player-gpui` e depois `refresh_traversal`), procurar no arquivo da árvore:
- um item de lista com "Titulo Teste, Artista Teste — Album Teste, 16/44.1, 0:02, atual";
- botões "Faixa anterior", "Tocar", "Próxima faixa", "Abrir arquivos ou pastas" e "Limpar a fila";
- um slider "Volume" com valor 50;
- o rótulo da faixa atual e o do relógio.

Registrar o que apareceu e o que faltou. Se a árvore vier vazia, o AccessKit não foi ativado pelo pedido do macos-use; registrar e repetir com o VoiceOver ligado (⌘F5) na medição 7.

- [ ] **Step 3: Gate e ponto de parada**

Run: `make gpui-fmt && make gpui-test && make test`
Expected: tudo verde. Commit só se o usuário pedir, com `feat(gpui): acrescentar papeis e rotulos de acessibilidade`.

---

### Task 11: O `.app` e o uso real com o device

**Files:**
- Create: `apps/gpui/app/Resources/Info.plist`
- Create: `apps/gpui/tools/make_app.sh`
- Modify: `Makefile` (alvo `gpui-app`, acrescentado no bloco do protótipo)

**Interfaces:**
- Produces: `make gpui-app`, que monta `apps/gpui/build/HogAudioGPUI.app` sem instalar.

- [ ] **Step 1: Bundle**

`apps/gpui/app/Resources/Info.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>HogAudioGPUI</string>
    <key>CFBundleDisplayName</key><string>hog-audio GPUI</string>
    <key>CFBundleIdentifier</key><string>local.hogaudio.gpui</string>
    <key>CFBundleExecutable</key><string>HogAudioGPUI</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
```

`apps/gpui/tools/make_app.sh`:

```bash
#!/usr/bin/env bash
# Monta o .app do protótipo em apps/gpui/build/. Não instala: o /Applications é do app SwiftUI,
# e sem Info.plist o macOS trata o binário como processo de segundo plano.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
app="$root/apps/gpui/build/HogAudioGPUI.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/apps/gpui/app/Resources/Info.plist" "$app/Contents/Info.plist"
cp "$root/apps/player/Resources/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
cp "$root/apps/gpui/target/release/hog-player-gpui" "$app/Contents/MacOS/HogAudioGPUI"
echo "app montado em $app"
```

No `Makefile`, dentro do bloco do protótipo:

```make
gpui-app: gpui
	@$(GPUI_DIR)/tools/make_app.sh
```

Run: `chmod +x apps/gpui/tools/make_app.sh && make gpui-app && plutil -lint apps/gpui/build/HogAudioGPUI.app/Contents/Info.plist`
Expected: "app montado em …" e `OK` do plutil.

- [ ] **Step 2: Avisar o usuário e pedir o ok**

Os Steps 3 a 5 **tomam o device de saída**. Mandar ao usuário: "Vou tocar pelo protótipo GPUI no fone, com volume baixo, por uns 5 minutos. Posso?". Esperar o sim. Antes, confirmar que o app SwiftUI está fechado (o hog é por PID, e os dois disputariam o device).

- [ ] **Step 3: Tocar, navegar, remover**

`open apps/gpui/build/HogAudioGPUI.app`. Depois, arrastar `testdata/t96_24.flac`, `testdata/t44_16.flac`, `testdata/meta_fixture.flac` e `testdata/t192_24.flac` para a janela.

Tocar e conferir:
- baixar o slider para 10% antes do play;
- a linha técnica diz "96 kHz · 24 bit · flac · hog ativo";
- o relógio anda;
- a linha atual mostra a onda sobre a miniatura.

Navegar:
- Próxima: troca para 44.1, a capa e o fundo cruzam, e a linha técnica acompanha;
- Anterior depois de 3 s rebobina;
- três Próxima rápidas carregam uma vez só: `tail ~/Library/Logs/hog-player-gpui.log` mostra uma linha "navegação 3".

Remover:
- selecionar a 2ª linha e apertar Delete, duas vezes seguidas (a 2ª e a que assumiu);
- as linhas certas saem, e a faixa atual continua tocando sem corte.

Deixar tocar até o fim: o auto-avanço passa para a próxima.

A `t192_24.flac` tem de falhar com a mensagem do core. O fone do Air M1 vai até 96 kHz; esse é o caso de falha de faixa.

- [ ] **Step 4: Perda do device**

Com o fone plugado e tocando, desplugar o fone:
- o aviso "o device de saída foi desconectado; a reprodução parou" aparece;
- nada toca sozinho.

Apertar Tocar: toca nas caixas no teto de volume.

Repetir desplugando no fim de uma faixa: o auto-avanço não toca nas caixas.

- [ ] **Step 5: Saída devolve o device**

Tocando, ⌘Q. Com `apps/gpui/bench/build/input_probe read <uid do fone>` (Task 12) ou com o Audio MIDI Setup, o fone volta ao rate de antes e fica sem dono de hog. Repetir com Ctrl+C no terminal (rodando o binário de `apps/gpui/build/HogAudioGPUI.app/Contents/MacOS/HogAudioGPUI`).

- [ ] **Step 6: Registrar e parar**

Anotar o resultado de cada item: passou, ou o que aconteceu. Isso vai para o resultado (Task 14). Se algo falhou, parar e trazer ao usuário antes das medições. Commit só se ele pedir, com `build(gpui): montar o .app do prototipo`.

---

### Task 12: Medidores externos

Os medidores servem aos dois apps por igual e não tocam em nenhum dos dois. São instrumentos: a verificação é conferir que medem o que dizem medir, contra um caso conhecido.

**Files:**
- Create: `apps/gpui/bench/window_timer.swift`
- Create: `apps/gpui/bench/input_probe.swift`
- Create: `apps/gpui/bench/README.md`

- [ ] **Step 1: `window_timer.swift`**

```swift
// Medição 4: do lançamento até a primeira janela do processo aparecer no WindowServer, pelo
// CGWindowList, igual para os dois apps. Termina o app com SIGTERM, que os dois tratam
// devolvendo o device.
//
//   swiftc -O -o apps/gpui/bench/build/window_timer apps/gpui/bench/window_timer.swift
//   apps/gpui/bench/build/window_timer <executável> [rodadas]
import CoreGraphics
import Foundation

func nowMs() -> Double { Double(DispatchTime.now().uptimeNanoseconds) / 1_000_000 }

func windowOnScreen(pid: pid_t) -> Bool {
    guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else {
        return false
    }
    return list.contains {
        ($0[kCGWindowOwnerPID as String] as? pid_t) == pid && ($0[kCGWindowLayer as String] as? Int) == 0
    }
}

let args = CommandLine.arguments
guard args.count >= 2 else {
    print("uso: window_timer <executável> [rodadas]")
    exit(2)
}
let executable = URL(fileURLWithPath: args[1])
let rounds = args.count >= 3 ? Int(args[2]) ?? 1 : 1
let limitMs = 15_000.0

for round in 1...rounds {
    let process = Process()
    process.executableURL = executable
    let start = nowMs()
    try process.run()
    var elapsed: Double?
    while nowMs() - start < limitMs {
        if windowOnScreen(pid: process.processIdentifier) {
            elapsed = nowMs() - start
            break
        }
        usleep(1_000)
    }
    process.terminate()
    process.waitUntilExit()
    guard let elapsed else {
        print("rodada \(round): a janela não apareceu em \(Int(limitMs / 1000)) s")
        exit(1)
    }
    print(String(format: "rodada %d: %.0f ms", round, elapsed))
    sleep(2)
}
```

- [ ] **Step 2: `input_probe.swift`**

```swift
// Medições 5, 8 e 9: gera o gesto com CGEvent e lê o device por Core Audio, só leitura.
// Precisa da permissão de Acessibilidade para o terminal (Ajustes > Privacidade).
//
//   swiftc -O -o apps/gpui/bench/build/input_probe apps/gpui/bench/input_probe.swift
//   input_probe list
//   input_probe read <uid>
//   input_probe slider <x1> <y1> <x2> <y2> <uid>
//   input_probe click <x> <y> <uid>
//   input_probe scroll <x> <y> <linhas> <vezes>
import CoreAudio
import CoreGraphics
import Foundation

func nowMs() -> Double { Double(DispatchTime.now().uptimeNanoseconds) / 1_000_000 }

func address(_ selector: AudioObjectPropertySelector,
             _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal,
             _ element: AudioObjectPropertyElement = kAudioObjectPropertyElementMain) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress(mSelector: selector, mScope: scope, mElement: element)
}

func read<T>(_ id: AudioObjectID, _ addr: AudioObjectPropertyAddress, _ initial: T) -> T? {
    var addr = addr
    var value = initial
    var size = UInt32(MemoryLayout<T>.size)
    return AudioObjectGetPropertyData(id, &addr, 0, nil, &size, &value) == noErr ? value : nil
}

func string(_ id: AudioObjectID, _ selector: AudioObjectPropertySelector) -> String? {
    var addr = address(selector)
    var value: Unmanaged<CFString>?
    var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
    guard AudioObjectGetPropertyData(id, &addr, 0, nil, &size, &value) == noErr else { return nil }
    return value?.takeRetainedValue() as String?
}

func devices() -> [AudioObjectID] {
    var addr = address(kAudioHardwarePropertyDevices)
    var size: UInt32 = 0
    AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size)
    var ids = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
    AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size, &ids)
    return ids
}

func device(uid: String) -> AudioObjectID? {
    devices().first { string($0, kAudioDevicePropertyDeviceUID) == uid }
}

/// Sob hog o "default output" aponta para outro device: por isso o device vem sempre pelo UID.
func volume(_ id: AudioObjectID) -> Float32? {
    read(id, address(kAudioDevicePropertyVolumeScalar, kAudioObjectPropertyScopeOutput), Float32(0))
        ?? read(id, address(kAudioDevicePropertyVolumeScalar, kAudioObjectPropertyScopeOutput, 1), Float32(0))
}

func running(_ id: AudioObjectID) -> Bool? {
    read(id, address(kAudioDevicePropertyDeviceIsRunningSomewhere), UInt32(0)).map { $0 != 0 }
}

func post(_ type: CGEventType, _ point: CGPoint) {
    CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: .left)?
        .post(tap: .cghidEventTap)
}

func requireDevice(_ uid: String) -> AudioObjectID {
    guard let id = device(uid: uid) else {
        print("device não encontrado: \(uid) (rode `input_probe list`)")
        exit(1)
    }
    return id
}

let args = Array(CommandLine.arguments.dropFirst())
switch args.first {
case "list":
    for id in devices() {
        print("\(id)\t\(string(id, kAudioDevicePropertyDeviceUID) ?? "?")\t\(string(id, kAudioObjectPropertyName) ?? "?")")
    }
case "read" where args.count == 2:
    let id = requireDevice(args[1])
    let rate = read(id, address(kAudioDevicePropertyNominalSampleRate), Float64(0)) ?? -1
    let hog = read(id, address(kAudioDevicePropertyHogMode), pid_t(0)) ?? -2
    print("volume \(volume(id).map { String($0) } ?? "sem controle")  rodando \(running(id).map { String($0) } ?? "?")  rate \(rate)  hog \(hog)")
case "slider" where args.count == 6:
    let id = requireDevice(args[5])
    let (x1, y1, x2, y2) = (Double(args[1])!, Double(args[2])!, Double(args[3])!, Double(args[4])!)
    post(.leftMouseDown, CGPoint(x: x1, y: y1))
    for step in 1...30 {
        let t = Double(step) / 30
        post(.leftMouseDragged, CGPoint(x: x1 + (x2 - x1) * t, y: y1 + (y2 - y1) * t))
        usleep(10_000)
    }
    post(.leftMouseUp, CGPoint(x: x2, y: y2))
    let released = nowMs()
    var last = volume(id)
    var lastChange = released
    while nowMs() - released < 2_000 {
        let current = volume(id)
        if current != last {
            last = current
            lastChange = nowMs()
        }
        usleep(1_000)
    }
    print(String(format: "soltar → último valor no hardware: %.1f ms (volume %@)", lastChange - released, last.map { String($0) } ?? "?"))
case "click" where args.count == 4:
    let id = requireDevice(args[3])
    let point = CGPoint(x: Double(args[1])!, y: Double(args[2])!)
    post(.leftMouseDown, point)
    post(.leftMouseUp, point)
    let clicked = nowMs()
    var stopped = running(id) == false
    while nowMs() - clicked < 3_000 {
        let now = running(id)
        if now == false { stopped = true }
        if stopped, now == true {
            print(String(format: "clique → device rodando de novo: %.1f ms", nowMs() - clicked))
            exit(0)
        }
        usleep(500)
    }
    print("o device não parou e voltou em 3 s (a troca manteve o device rodando?)")
    exit(1)
case "scroll" where args.count == 5:
    let point = CGPoint(x: Double(args[1])!, y: Double(args[2])!)
    let (lines, times) = (Int32(args[3])!, Int(args[4])!)
    CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: point, mouseButton: .left)?
        .post(tap: .cghidEventTap)
    for _ in 0..<times {
        CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1, wheel1: lines, wheel2: 0, wheel3: 0)?
            .post(tap: .cghidEventTap)
        usleep(16_000)
    }
default:
    print("uso: input_probe list | read <uid> | slider x1 y1 x2 y2 <uid> | click x y <uid> | scroll x y linhas vezes")
    exit(2)
}
```

- [ ] **Step 3: Compilar e conferir contra casos conhecidos, sem device**

Run:
```bash
mkdir -p apps/gpui/bench/build
swiftc -O -o apps/gpui/bench/build/window_timer apps/gpui/bench/window_timer.swift
swiftc -O -o apps/gpui/bench/build/input_probe apps/gpui/bench/input_probe.swift
apps/gpui/bench/build/window_timer /System/Applications/TextEdit.app/Contents/MacOS/TextEdit 2
apps/gpui/bench/build/input_probe list
apps/gpui/bench/build/input_probe read <uid do fone, da lista>
```
Expected:
- o `window_timer` imprime dois tempos do TextEdit, na casa das centenas de ms, e fecha o TextEdit;
- o `list` mostra o fone (no M1, `BuiltInHeadphoneOutputDevice`);
- o `read` mostra o volume atual, `rodando false`, o rate e `hog -1`.

`apps/gpui/bench/build/` fica fora do git: acrescentar `bench/build/` ao `apps/gpui/.gitignore`.

- [ ] **Step 4: `README.md` do bench**

`apps/gpui/bench/README.md`, com o protocolo de cada medição (os comandos da Task 13), a permissão de Acessibilidade exigida pelo `input_probe`, e o aviso de que as medições 1, 2, 5, 6, 8 (⌘Q) e 9 tomam o device. Sem tabelas.

- [ ] **Step 5: Ponto de parada**

Mostrar ao usuário a saída do Step 3. Commit só se ele pedir, com `test(gpui): acrescentar medidores externos de janela e de entrada`.

---

### Task 13: As medições

Cada medição que toma o device **só roda depois de avisar o usuário e ter o ok**, uma de cada vez. `powermetrics` e `purge` pedem `sudo`: o usuário roda com `! <comando>`, ou autoriza.

Os números brutos vão para `docs/superpowers/specs/2026-09-27-prototipo-gpui-medicoes.md`, uma seção por medição, com as três rodadas e a mediana, sem tabelas.

**Montagem**, conferida antes da primeira medição:
- MacBook Air M1 na bateria, com brilho fixo (anotar o valor) e só o Terminal aberto.
- Controle: `/Applications/HogAudio.app` (main `a11fe30`). Protótipo: `apps/gpui/build/HogAudioGPUI.app`.
- Um app aberto por vez.
- Janela de 1280×800 pt nos dois, ajustada com o macos-use, lendo o tamanho pela árvore de acessibilidade.
- A biblioteca do usuário, 1.157 faixas com capa, já baixada localmente. **Perguntar o caminho ao usuário** e conferir com `find <pasta> -name '*.flac' | head` e com `ls -lO` (sem `dataless`).
- O executável do controle é `/Applications/HogAudio.app/Contents/MacOS/HogAudio`; o do protótipo, `apps/gpui/build/HogAudioGPUI.app/Contents/MacOS/HogAudioGPUI`.

- [ ] **Medição 1: custo parado**

Com a biblioteca na fila (o caminho como argumento do executável), uma faixa tocada e pausada, a janela na frente e sem mexer por 60 s:

```bash
sudo powermetrics --samplers tasks --show-process-energy -i 1000 -n 60 > m1-<app>-<rodada>.txt
```

Extrair do arquivo a linha do processo, em cada amostra: CPU ms/s, "interrupt wakeups", "idle wakeups" e "energy impact". Registrar a mediana das 60 amostras por rodada.

Repetir com a janela minimizada. Três rodadas por app, alternando a ordem.

- [ ] **Medição 2: tocando**

Uma faixa 24/96 da biblioteca tocando por 60 s, janela na frente:

```bash
sudo powermetrics --samplers tasks,gpu_power --show-process-energy -i 1000 -n 60
```

As mesmas métricas mais a potência da GPU.

- [ ] **Medição 3: memória**

`footprint <pid> | grep -i footprint` em três momentos:
- logo depois de abrir, com a janela vazia;
- com a biblioteca na fila e todas as linhas preenchidas (esperar a CPU do processo voltar ao repouso);
- depois de rolar a fila inteira, com `input_probe scroll <x> <y> -5 400` sobre a fila.

- [ ] **Medição 4: abertura**

Frio, cinco rodadas por app, com `sudo purge` antes de cada uma:

```bash
apps/gpui/bench/build/window_timer <executável> 1
```

Quente, cinco rodadas seguidas: `window_timer <executável> 5`.

Conferência: `xcrun xctrace record --template 'App Launch' --launch -- <executável>`, uma rodada por app, anotando o tempo até o primeiro frame que o Instruments mostra.

- [ ] **Medição 5: resposta**

Pegar as coordenadas do trilho do slider e de uma linha da fila pelo macos-use (os campos `x y w h` da árvore).

- **Slider:** `input_probe slider <x1> <y> <x2> <y> <uid>`, dez vezes por app, tocando.
- **Hitches do slider:** `xcrun xctrace record --template 'Animation Hitches' --attach <pid> --time-limit 10s --output m5-slider-<app>.trace`, rodando o `input_probe slider` em paralelo. Exportar com `xcrun xctrace export --input <trace> --toc`, achar a tabela de hitches e contá-los. Se o template não enxergar os quadros do GPUI (Metal próprio), usar `--template 'Metal System Trace'` e medir o p99 do intervalo entre quadros.
- **Clique na fila:** `input_probe click <x> <y> <uid>` numa linha de formato diferente da atual, dez vezes por app. O device tem de parar e voltar.
- **Delete:** selecionar uma linha do meio e apertar Delete, dez vezes por app, conferindo pela árvore que saiu a linha certa.
- **Rolagem:** `input_probe scroll <x> <y> -5 400` com o `Animation Hitches` gravando.
- **Encher a fila:** lançar o app com o caminho da biblioteca como argumento e medir, com `top -l 0 -s 1 -pid <pid> -stats cpu`, o tempo até a CPU ficar abaixo de 2% por 3 s seguidos. Registrar à parte, porque os leitores de tag são outros (AVFoundation e lofty).
- **Fallback:** se o `input_probe read` mostrar que o HAL recusa ler o device em hog de outro processo, trocar pelas linhas de transição: `log show --predicate 'subsystem == "local.hogaudio.player"' --last 5m` no SwiftUI e `~/Library/Logs/hog-player-gpui.log` no protótipo.

- [ ] **Medição 6: áudio sob estresse**

Cinco minutos tocando uma faixa 24/96 da biblioteca (repetir a faixa se precisar). Enquanto toca:
- `input_probe scroll` em laço;
- redimensionar a janela pelo macos-use a cada 10 s;
- `input_probe slider` a cada 15 s;
- Próxima a cada minuto.

Gravar em paralelo `xcrun xctrace record --template 'Audio System Trace' --attach <pid> --time-limit 300s`.

Critério:
- a linha técnica termina sem "underruns";
- o trace não mostra `ProcessorOverload`.

- [ ] **Medição 7: VoiceOver**

Com o VoiceOver ligado (⌘F5) e o Accessibility Inspector aberto, percorrer:
- a fila: cada linha lê título, artista, duração e selo;
- os botões de transporte, Abrir e Limpar: todos têm nome;
- o slider: ajusta com VO + setas;
- Delete e ⌘O sem mouse;
- o aviso de erro, provocado soltando um arquivo que não é áudio, é alcançável.

Sim ou não por item, por app, com observações. Registrar a diferença do Delete (a janela inteira no GPUI, a lista em foco no SwiftUI).

- [ ] **Medição 8: integração com o Mac**

Sim ou não por app, com observações:
- ⌘O no menu e como atalho;
- filtro de tipos no painel de abrir (o GPUI não filtra: registrar);
- arrastar três arquivos e uma pasta de uma vez do Finder;
- barra de título transparente;
- tamanho mínimo;
- tooltip no selo;
- Delete e espaço;
- ⌘Q e Ctrl+C devolvem o device: `input_probe read <uid>` depois mostra `hog -1` e o rate de antes.

- [ ] **Medição 9: pânico**

Só no protótipo:

```bash
cd apps/gpui && cargo build --release -p hog-player-gpui --features panic-probe
```

Tocar uma faixa 24/96 do fixture, anotar o `input_probe read <uid>` e usar o menu Depurar > Provocar pânico. Registrar:
- se o processo abortou;
- o que o stderr mostrou ("pânico: device devolvido" ou o teto);
- o `input_probe read <uid>` logo depois (hog e rate).

Reconstruir sem a feature ao terminar.

- [ ] **Medição 10: build**

- **Protótipo:** `cd apps/gpui && cargo clean && time cargo build --release -p hog-player-gpui`, `du -sh apps/gpui/build/HogAudioGPUI.app` depois do `make gpui-app`, e o número de crates da Task 0.
- **Controle:** `rm -rf apps/player/.build && (cd rust && cargo clean) && time make app`, e `du -sh apps/player/HogAudio.app`. Isso limpa o build do core, não o código: o `make test` seguinte reconstrói. Avisar o usuário antes.
- Se cada um exige o Xcode completo (o GPUI, pelo Metal Toolchain; o SwiftUI, pelo `swift build`).

- [ ] **Ponto de parada**

Mostrar ao usuário o arquivo de medições preenchido. Commit só se ele pedir, com `docs(gpui): registrar medicoes do prototipo`.

---

### Task 14: Resultado

**Files:**
- Create: `docs/superpowers/specs/2026-09-27-prototipo-gpui-resultado.md`
- Modify: `~/work/knowledges/projects/vitral/vitral-visao-e-decisoes.md` (seções 5, 7 e 8)
- Modify: a memória do hog-audio e a do vitral (com as ferramentas de memória)

- [ ] **Step 1: O doc de resultado**

Enxuto, em pt-BR, sem tabelas, no molde de `2026-09-26-observable-e-device-resultado.md`:
- **O que foi construído:** workspace, crates e o que foi aproveitado do Pawse.
- **Bit-perfect:** o `git diff main -- rust/` vazio, as versões iguais às do core e o fato de o protótipo só chamar a API pública.
- **As medições 1 a 10:** a mediana e a variação entre rodadas, para os dois apps.
- **Os sim ou não das medições 7 e 8**, com as observações.
- **O veredito pela regra combinada:** um eliminatório reprovado basta, com o número que o reprovou. Se nenhum reprovou, dizer que o GPUI passou nos eliminatórios e listar o que classifica a favor e contra.
- **As diferenças deliberadas**, cada uma com a razão:
  - lista de extensões no lugar de `UTType.audio`;
  - Delete na janela inteira;
  - timer sem folga;
  - `lofty` no lugar do AVFoundation;
  - painel de abrir sem filtro;
  - ⌘O também no menu.
- **O que ficou de fora** e o que um passo seguinte pediria. Se o GPUI vencer: a ADR que substitui o "Swift é a interface" e a derivação do vitral.

- [ ] **Step 2: O vitral**

No doc do vitral:
- **Seção 5:** o que o passo 0 provou sobre compilar fora do workspace (patches, lockfile semeado, `font-kit`, Metal Toolchain).
- **Seção 7:** a acessibilidade medida.
- **Seção 8:** o spike marcado como feito, com o link para o resultado.

- [ ] **Step 3: Memória e parada final**

Atualizar a memória do hog-audio (estado do protótipo e veredito) e a do vitral. Mostrar o resultado ao usuário. Commit só se ele pedir, com `docs(gpui): registrar resultado da comparacao GPUI x SwiftUI`.

