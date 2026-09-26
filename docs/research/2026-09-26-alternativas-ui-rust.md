# Alternativas de UI em Rust para o hog-audio

Pesquisa de 26/09/2026. A pergunta: vale eliminar o SwiftUI e fazer a UI inteira em Rust, no mesmo processo do engine em hog mode, num MacBook Air M1?

A análise do egui no spotifast já está em `~/work/learning-playground/rust-playground/referencias/fastpotify-ui-egui.md` e não se repete aqui.

## Resposta

Nenhuma UI em Rust iguala o SwiftUI nos dois critérios de maior peso: integração com o macOS (controles, acessibilidade e texto do próprio AppKit) e maturidade. Os gargalos da UI atual são de uso, não do framework: `ObservableObject` publicando a 10 Hz e `id: \.offset` na fila. Se a meta for UI inteira em Rust, servem o GPUI e o Slint. O GPUI é o mais integrado (Metal direto, Core Text, NSMenu, NSOpenPanel, drop do Finder) e já tem um player com hog mode no mesmo processo; o Slint é o mais estável (API 1.x, empresa por trás) e o que melhor dorme parado. Recomendo o GPUI como alvo em Rust, condicionado a um protótipo que meça o custo parado e o VoiceOver, com o Slint como segundo finalista; o egui funciona, mas perde para os dois em integração e em energia com o relógio rodando.

## Critérios

1. Integração com o macOS: backend de renderização, nativo ou desenhado, menu, cromo, drop, diálogo, atalhos, tooltip, acessibilidade e texto.
2. Desempenho e energia: modelo de redesenho, custo parado e com relógio de 4–10 Hz, lista de mais de 1.000 linhas, imagens, blur e gradientes.
3. Convivência com o engine: dono da main thread, como acordar a UI, runtime async, pânico e pilha de áudio própria.
4. Maturidade: versão e data, 1.0, quebras de API, mantenedor e financiamento, licença e apps no macOS.
5. Liberdade visual: canvas, shaders e temas.
6. Testabilidade: UI headless e estado sem janela.
7. Custo de build: dependências e código nativo.

Tecnologia web (webviews, Tauri, Electron, Dioxus e o renderer Blitz) ficou fora por decisão do usuário.

Convenção: bullet com link é fato citado. "Inferência:" é dedução minha. "Sem fonte:" é o que não achei em fonte primária.

Fatos que valem para vários candidatos:
- A Apple declarou o OpenGL obsoleto no macOS 10.14 e manda usar Metal ([Apple](https://developer.apple.com/library/archive/documentation/GraphicsImaging/Conceptual/OpenGL-MacProgGuide/opengl_intro/opengl_intro.html)). Isso pesa contra o `glow` do spotifast, o FemtoVG-GL (padrão do Slint) e o Vizia.
- O winit exige a main thread no macOS ([event_loop.rs#L221](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/macos/event_loop.rs#L221)). egui, Iced, Slint, Freya, Xilem, Floem e Vizia ficam donos dela. GPUI e Makepad têm integração própria com o AppKit e também exigem a main thread.
- Desde o Rust 1.81, pânico que atravessa ABI `"C"` aborta ([blog Rust 1.81](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/)). O winit 0.30 no macOS captura o pânico nos observers e relança com `resume_unwind` ao sair do `run` ([event_loop.rs#L436-L458](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/macos/event_loop.rs#L436-L458)).
- Inferência: em todos os candidatos, um pânico na UI mata o processo e o engine junto. A diferença é se sobra tempo para soltar o hog mode e restaurar o formato do device.

---

## SwiftUI (linha de base)

Vem com o SO. O app exige macOS 14 (`apps/player/Package.swift:16`). O SO atual é o macOS 27 "Golden Gate", com SDK no Xcode 27 ([release notes](https://developer.apple.com/documentation/macos-release-notes/macos-27-release-notes)).

**1. Integração**
- Roda hospedado em AppKit: `NSHostingView` é "An AppKit view that hosts a SwiftUI view hierarchy" ([doc](https://developer.apple.com/documentation/swiftui/nshostingview)).
- Menus com `CommandMenu` ([doc](https://developer.apple.com/documentation/swiftui/commandmenu)).
- `help(_:)` vira tooltip e dica de acessibilidade ([doc](https://developer.apple.com/documentation/swiftui/view/help(_:))).
- `onDeleteCommand` responde a ⌫ e ⌦ ([doc](https://developer.apple.com/documentation/swiftui/view/ondeletecommand(perform:))).
- `windowResizability` desde o macOS 13 ([doc](https://developer.apple.com/documentation/swiftui/scene/windowresizability(_:))).
- `dropDestination` e `fileImporter` cobrem drop e diálogo ([doc](https://developer.apple.com/documentation/swiftui/view/dropdestination(for:action:istargeted:)), [doc](https://developer.apple.com/documentation/swiftui/view/fileimporter(ispresented:allowedcontenttypes:allowsmultipleselection:oncompletion:))).
- Acessibilidade embutida: "provides basic accessibility labels and values by default" ([doc](https://developer.apple.com/documentation/swiftui/accessibility-fundamentals)).
- `NSViewRepresentable` embute qualquer NSView, como uma `NSTableView` ([doc](https://developer.apple.com/documentation/swiftui/nsviewrepresentable)).
- Sem fonte: o motor de texto do `Text` e se a `List` no macOS usa `NSTableView` por baixo.

**2. Desempenho e energia**
- Com `@Observable`, a view só atualiza quando o body lê a propriedade que mudou. Com `ObservableObject`, atualiza a cada `@Published` ([doc](https://developer.apple.com/documentation/swiftui/migrating-from-the-observable-object-protocol-to-the-observable-macro)).
- O app usa `ObservableObject` com um `Timer(timeInterval: 0.1)` sem `tolerance` (`apps/player/Sources/HogPlayerKit/PlayerViewModel.swift:103`).
- A fila é `ForEach(Array(model.queue.enumerated()), id: \.offset)` (`apps/player/Sources/HogPlayer/ContentView.swift:276`).
- Inferência: durante a reprodução, o `ContentView` reavalia cerca de 10 vezes por segundo e refaz um array O(n) da fila. O id por posição muda a identidade de todas as linhas depois de uma remoção.
- `List` e `Table` coletam todos os IDs de uma vez ([WWDC23 10160](https://developer.apple.com/videos/play/wwdc2023/10160/)). `LazyVStack` cria itens sob demanda ([doc](https://developer.apple.com/documentation/swiftui/lazyvstack)).
- `Timer.tolerance` é 0 por padrão, e dar tolerância economiza energia ([doc](https://developer.apple.com/documentation/foundation/timer/tolerance)).
- O app já pré-desfoca a capa com Core Image fora da main thread (`ContentView.swift:49`). `Material` só desfoca conteúdo do próprio app ([doc](https://developer.apple.com/documentation/swiftui/material)).
- Sem fonte: números da Apple de custo parado ou energia do SwiftUI.

**3. Convivência com o engine**
- `View` é `@MainActor` ([doc](https://developer.apple.com/documentation/swiftui/view)). Não há tokio.
- O uniffi 0.32 faz `catch_unwind` e converte o pânico em erro ([rustcalls.rs#L159-L163](https://github.com/mozilla/uniffi-rs/blob/v0.32.0/uniffi_core/src/ffi/rustcalls.rs#L159-L163)).
- Mas `snapshot()` é gerado com `try! rustCall` (`apps/player/Sources/HogAudioBindings/hog_audio.swift:893-899`). Um pânico ali derruba o app sem passar por `model.shutdown()` (`apps/player/Sources/HogPlayer/HogPlayerApp.swift:47-50`).
- Inferência: o custo da FFI a 10 Hz, um `RustBuffer` pequeno por poll, é desprezível.

**4. Maturidade**
- Proprietário da Apple e atrelado ao SO. O WWDC26 trouxe `reorderable()` e `@State` lazy com backport ao macOS 14 ([WWDC26 269](https://developer.apple.com/videos/play/wwdc2026/269/)).
- O macOS 27 ainda corrige drops em `List` ([release notes](https://developer.apple.com/documentation/macos-release-notes/macos-27-release-notes)).

**5. Visual**
- `Canvas`, `ShaderLibrary` (Metal) e `layerEffect` desde o macOS 14 ([doc](https://developer.apple.com/documentation/swiftui/shaderlibrary), [doc](https://developer.apple.com/documentation/swiftui/view/layereffect(_:maxsampleoffset:isenabled:))).

**6. Testes**
- Swift Testing cobre estado sem janela ([doc](https://developer.apple.com/documentation/testing)). O app tem 68 `@Test` em `apps/player/Tests/HogPlayerKitTests`.
- Teste de UI exige XCUITest com o app rodando ([doc](https://developer.apple.com/documentation/xcuiautomation)).

**7. Build**
- Dois toolchains em três etapas: `cargo build`, `uniffi-bindgen` e `swift build` (`Makefile:15`, `Makefile:40`, `Makefile:107`).

**Para o hog-audio:** é a referência de qualidade. Cobre a tela inteira com controles nativos e acessibilidade automática, e os gargalos achados se corrigem dentro dele.

---

## egui / eframe 0.36.2 (linha de base)

0.36.2 saiu em 08/09/2026, 0.35.0 em 25/06/2026 e 0.34.0 em 26/03/2026 ([crates.io](https://crates.io/api/v1/crates/eframe)). Licença MIT OR Apache-2.0.

**1. Integração**
- O renderer padrão do eframe 0.36 é wgpu, que usa Metal no macOS. O `glow` (OpenGL) é opt-in ([Cargo.toml#L27-L37](https://github.com/emilk/egui/blob/0.36.2/crates/eframe/Cargo.toml#L27-L37)). O wgpu virou padrão na 0.34.0 ([CHANGELOG#L80](https://github.com/emilk/egui/blob/0.36.2/crates/eframe/CHANGELOG.md#L80)).
- O spotifast usa `glow` (`~/work/audio-toolbox/spotifast/Cargo.toml:37-40`), ou seja, OpenGL obsoleto.
- Aparência nativa está fora do objetivo: "If you want a GUI that looks native, egui is not for you" ([README#L156](https://github.com/emilk/egui/blob/0.36.2/README.md#L156)).
- Não há API de menu. O menu padrão vem do winit ([menu.rs#L66-L73](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/macos/menu.rs#L66-L73)), e um NSMenu próprio exige o crate `muda` ([README](https://github.com/tauri-apps/muda/blob/dev/README.md)).
- Cromo: `with_fullsize_content_view`, `with_titlebar_shown(false)` e `with_min_inner_size` ([viewport.rs#L465-L544](https://github.com/emilk/egui/blob/0.36.2/crates/egui/src/viewport.rs#L465-L544)).
- Drop do Finder via `RawInput::dropped_files` ([raw_input.rs#L62-L81](https://github.com/emilk/egui/blob/0.36.2/crates/egui/src/data/input/raw_input.rs#L62-L81)). O winit 0.30 entrega um evento por arquivo, sem posição ([event.rs#L176-L192](https://github.com/rust-windowing/winit/blob/v0.30.13/src/event.rs#L176-L192)).
- Não tem diálogo de abrir. O FAQ aponta o `rfd` ([README#L271-L273](https://github.com/emilk/egui/blob/0.36.2/README.md#L271-L273)), que usa NSOpenPanel ([panel_ffi.rs](https://github.com/PolyMeilex/rfd/blob/0.17.2/src/backend/macos/file_dialog/panel_ffi.rs)).
- ⌘ via `Modifiers::COMMAND` ([modifiers.rs#L114-L120](https://github.com/emilk/egui/blob/0.36.2/crates/egui/src/data/input/modifiers.rs#L114-L120)). Tooltip via `on_hover_text` ([response.rs#L665](https://github.com/emilk/egui/blob/0.36.2/crates/egui/src/response.rs#L665)).
- AccessKit vem ligado ([README#L276](https://github.com/emilk/egui/blob/0.36.2/README.md#L276)). O 0.36.2 fixa accesskit 0.24.1 e accesskit_winit 0.32 por causa do kittest ([Cargo.toml#L73-L75](https://github.com/emilk/egui/blob/0.36.2/Cargo.toml#L73-L75)). Live regions estão abertas desde 2023 ([#2647](https://github.com/emilk/egui/issues/2647)).
- Inferência: não há caminho oficial para anunciar a troca de faixa ao VoiceOver.
- Texto: rasterizador próprio (skrifa + vello_cpu, com hinting) desde a 0.34 ([epaint CHANGELOG#L25-L55](https://github.com/emilk/egui/blob/0.36.2/crates/epaint/CHANGELOG.md#L25-L55)). As fontes padrão são embutidas, não as do sistema ([README#L360-L366](https://github.com/emilk/egui/blob/0.36.2/README.md#L360-L366)). Bidi está aberto desde 2021 ([#1016](https://github.com/emilk/egui/issues/1016)).

**2. Desempenho e energia**
- "an immediate mode GUI does a full layout each frame", com "1-2 ms per frame"; só repinta com interação ou animação ([README#L230-L234](https://github.com/emilk/egui/blob/0.36.2/README.md#L230-L234)).
- Não há repaint parcial: o wgpu limpa a janela inteira com `LoadOp::Clear` ([winit.rs#L659](https://github.com/emilk/egui/blob/0.36.2/crates/egui-wgpu/src/winit.rs#L659)).
- Não há cache de tesselação, por decisão ([context.rs#L2866-L2868](https://github.com/emilk/egui/blob/0.36.2/crates/egui/src/context.rs#L2866-L2868)).
- Parado, o loop fica em `ControlFlow::Wait` ([run.rs#L245-L254](https://github.com/emilk/egui/blob/0.36.2/crates/eframe/src/native/run.rs#L245-L254)).
- O busy-loop que motivou o fork do spotifast saiu no 0.36.2 ([PR #8398](https://github.com/emilk/egui/pull/8398)). A medição da PR é do Wayland.
- Inferência: cada tick de 4–10 Hz roda a UI inteira, re-tessela tudo e redesenha a janela toda.
- `ScrollArea::show_rows` desenha só as linhas visíveis, com altura fixa ([scroll_area.rs#L966-L1020](https://github.com/emilk/egui/blob/0.36.2/crates/egui/src/containers/scroll_area.rs#L966-L1020)). Isso corrige a análise do fastpotify, que diz que a virtualização "não existe pronta no egui".
- O loader de imagens do `egui_extras` abre uma thread por imagem ([image_loader.rs#L84-L100](https://github.com/emilk/egui/blob/0.36.2/crates/egui_extras/src/loaders/image_loader.rs#L84-L100)). Mipmap só existe no glow ([textures.rs#L160-L178](https://github.com/emilk/egui/blob/0.36.2/crates/epaint/src/textures.rs#L160-L178)).
- Não há blur de imagem, só o blur de sombra ([shadow.rs#L3-L26](https://github.com/emilk/egui/blob/0.36.2/crates/epaint/src/shadow.rs#L3-L26)). Gradiente via `Shape::gradient_rect` ([shape.rs#L300-L306](https://github.com/emilk/egui/blob/0.36.2/crates/epaint/src/shapes/shape.rs#L300-L306)).
- Issue aberta no macOS: CPU acima de 50% com a janela parcialmente coberta, num M2 ([#7059](https://github.com/emilk/egui/issues/7059)).

**3. Convivência com o engine**
- `Context` é `Send + Sync`, e `request_repaint` chamado de outra thread acorda a UI ([context.rs#L1808-L1821](https://github.com/emilk/egui/blob/0.36.2/crates/egui/src/context.rs#L1808-L1821)). Não há tokio.
- Com ⌘Q, "run_app_on_demand doesn't return (despite its name)" ([run.rs#L279](https://github.com/emilk/egui/blob/0.36.2/crates/eframe/src/native/run.rs#L279)).
- Inferência: soltar o hog tem de ficar em `App::on_exit`, nunca num `Drop` depois de `run_native`.
- Não há dependência de áudio ([crates.io](https://crates.io/api/v1/crates/eframe/0.36.2/dependencies)).

**4. Maturidade**
- Pré-1.0: "New releases will have breaking changes" ([README#L116](https://github.com/emilk/egui/blob/0.36.2/README.md#L116)). A 0.34 trocou `App::update` por `logic` + `ui` ([CHANGELOG](https://github.com/emilk/egui/blob/0.36.2/CHANGELOG.md)).
- Mantido por Emil Ernerfeldt, com patrocínio da Rerun ([README#L358](https://github.com/emilk/egui/blob/0.36.2/README.md#L358)).
- Em produção no Rerun Viewer 0.38.1, com binário macOS arm64 ([release](https://github.com/rerun-io/rerun/releases/tag/0.38.1)).
- O spotifast depende de forks do egui e do winit (`spotifast/Cargo.toml:226-251`). Dos patches, só o de bidi importa num app só macOS, e ele ainda é draft ([PR #8577](https://github.com/emilk/egui/pull/8577)).

**5. Visual**
- `Painter` e `Shape::Mesh`. Shaders próprios via `egui_wgpu::Callback` ([renderer.rs#L29-L33](https://github.com/emilk/egui/blob/0.36.2/crates/egui-wgpu/src/renderer.rs#L29-L33)).
- `Style` e `Visuals` são "not yet as powerful as CSS" ([README#L254-L256](https://github.com/emilk/egui/blob/0.36.2/README.md#L254-L256)).

**6. Testes**
- `egui_kittest` 0.36.2 faz queries pela árvore AccessKit e snapshot em wgpu ([README](https://github.com/emilk/egui/blob/0.36.2/crates/egui_kittest/README.md)).
- `Context::run_ui` roda um frame sem janela.

**7. Build**
- O eframe 0.36.2 tem 15 dependências obrigatórias no macOS ([crates.io](https://crates.io/api/v1/crates/eframe/0.36.2/dependencies)).
- Inferência: não há C/C++ no caminho macOS (glutin e wgpu-hal usam bindings Rust); o grafo transitivo não foi resolvido com cargo.
- Um usuário mediu no Windows binário LTO de 5,62 MB com glow e 11,3 MB com wgpu ([#7761](https://github.com/emilk/egui/issues/7761)). Não é medição do projeto.

**Para o hog-audio:** funciona e dá o visual do spotifast, desde que use wgpu e não glow. Perde para GPUI e Slint em integração (sem menu, diálogo e texto do sistema) e em energia com o relógio (layout, tesselação e redesenho da janela inteira por tick). Não é finalista.

---

## Iced 0.14.0

0.14.0 saiu em 07/12/2025 e não houve release em 2026 ([release](https://github.com/iced-rs/iced/releases/tag/0.14.0)). O master está em 0.15.0-dev, 590 commits à frente, com winit por fork git ([Cargo.toml#L244-L245](https://github.com/iced-rs/iced/blob/10e9b995eb5af74dbac08ed62b01971fe7e495b2/Cargo.toml#L244-L245)). Licença MIT.

**1. Integração**
- wgpu (Metal) com fallback tiny-skia ([README#L39-L41](https://github.com/iced-rs/iced/blob/0.14.0/README.md#L39-L41)). Desenho próprio.
- Sem fonte: API de menu nativo; a busca no código da 0.14.0 e do master não achou.
- Titlebar transparente via `PlatformSpecific` ([macos.rs#L5-L12](https://github.com/iced-rs/iced/blob/0.14.0/core/src/window/settings/macos.rs#L5-L12)).
- Drop do Finder via `FileDropped` ([event.rs#L48-L66](https://github.com/iced-rs/iced/blob/0.14.0/core/src/window/event.rs#L48-L66)).
- Diálogo via `rfd` ([examples/editor](https://github.com/iced-rs/iced/blob/0.14.0/examples/editor/src/main.rs#L260)).
- Sem acessibilidade: a issue está aberta desde 2020 ([#552](https://github.com/iced-rs/iced/issues/552)) e a PR de AccessKit é draft ([#3111](https://github.com/iced-rs/iced/pull/3111)).
- Texto via cosmic-text ([graphics/Cargo.toml#L33](https://github.com/iced-rs/iced/blob/0.14.0/graphics/Cargo.toml#L33)).

**2. Desempenho e energia**
- "Reactive rendering" desde a 0.14 ([PR #2662](https://github.com/iced-rs/iced/pull/2662)).
- Cada mensagem reconstrói a UI inteira e refaz o layout ([lib.rs#L1221-L1253](https://github.com/iced-rs/iced/blob/0.14.0/winit/src/lib.rs#L1221-L1253)).
- Dano parcial só existe no tiny-skia ([compositor.rs#L176-L213](https://github.com/iced-rs/iced/blob/0.14.0/tiny_skia/src/window/compositor.rs#L176-L213)).
- `time::every` só existe com tokio ou smol ([time.rs#L4-L13](https://github.com/iced-rs/iced/blob/0.14.0/src/time.rs#L4-L13)).
- Não há lista virtualizada no upstream; ela só existe no fork da pop-os ([#3429](https://github.com/iced-rs/iced/issues/3429)).
- Não há blur de imagem, e `Gradient` só é linear ([gradient.rs#L11](https://github.com/iced-rs/iced/blob/0.14.0/core/src/gradient.rs#L11)).

**3. Convivência com o engine**
- O executor padrão é thread-pool; tokio é opcional ([Cargo.toml#L54-L59](https://github.com/iced-rs/iced/blob/0.14.0/Cargo.toml#L54-L59)). Não há `catch_unwind` nem áudio.

**4. Maturidade**
- "Iced is currently experimental software" ([README#L44](https://github.com/iced-rs/iced/blob/0.14.0/README.md#L44)). Tem mantenedor quase único e patrocínio da Kraken ([README#L184-L186](https://github.com/iced-rs/iced/blob/0.14.0/README.md#L184-L186)).
- O Halloy, app de vitrine, roda sobre o fork `squidowl/iced` ([Cargo.toml#L168-L171](https://github.com/squidowl/halloy/blob/dd41280c0a3bdda90c04752114d26a8a9c898ffa/Cargo.toml#L168-L171)).

**5 a 7**
- Tem `canvas` e o widget `Shader`, com pipeline wgpu próprio ([shader.rs](https://github.com/iced-rs/iced/blob/0.14.0/widget/src/shader.rs)).
- `iced_test` roda headless com snapshot ([test/src/lib.rs#L1](https://github.com/iced-rs/iced/blob/0.14.0/test/src/lib.rs#L1)).
- Sem C/C++.

**Para o hog-audio:** fora. Não tem acessibilidade, menu nativo nem lista virtualizada no upstream, e o último release tem 10 meses.

---

## Slint 1.18.1

1.18.1 saiu em 21/09/2026, 1.17.0 em 24/06/2026 e 1.16.0 em 16/04/2026 ([crates.io](https://crates.io/api/v1/crates/slint)).

**1. Integração**
- O backend é winit. O renderer padrão é FemtoVG sobre OpenGL ([Cargo.toml#L23-L31](https://github.com/slint-ui/slint/blob/v1.18.1/api/rs/slint/Cargo.toml#L23-L31)).
- `renderer-skia` desenha em Metal via wgpu ([skia/lib.rs#L274-L285](https://github.com/slint-ui/slint/blob/v1.18.1/internal/renderers/skia/lib.rs#L274-L285)), e `renderer-femtovg-wgpu` também usa Metal ([backends_and_renderers.mdx#L93-L102](https://github.com/slint-ui/slint/blob/v1.18.1/docs/astro/src/content/docs/guide/backends-and-renderers/backends_and_renderers.mdx#L93-L102)).
- Desenho próprio. Em alvos Apple, o estilo `native` é o `cupertino`, que "emulate the style used by macOS" ([style.mdx#L15-L29](https://github.com/slint-ui/slint/blob/v1.18.1/docs/astro/src/content/docs/reference/std-widgets/style.mdx#L15-L29)).
- `MenuBar` vira a barra de menus do macOS ([builtin_elements.rs#L1923-L1935](https://github.com/slint-ui/slint/blob/v1.18.1/internal/compiler/builtin_elements.rs#L1923-L1935)), com atalhos como `KeyAccelerator` ([CHANGELOG#L515](https://github.com/slint-ui/slint/blob/v1.18.1/CHANGELOG.md#L515)).
- A titlebar transparente só é possível pelo hook `unstable-winit-030`, fora da garantia de estabilidade ([selector/api.rs#L170-L178](https://github.com/slint-ui/slint/blob/v1.18.1/internal/backends/selector/api.rs#L170-L178)).
- O backend winit da 1.18.1 não trata drop do Finder. `DragArea` e `DropArea` funcionam só dentro da janela ([CHANGELOG#L427](https://github.com/slint-ui/slint/blob/v1.18.1/CHANGELOG.md#L427)). O suporte nativo foi mergeado só na branch do winit 0.31 ([PR #12294](https://github.com/slint-ui/slint/pull/12294)).
- Inferência: dá para capturar `DroppedFile` pelo mesmo hook instável.
- Não tem diálogo de arquivos; use `rfd`.
- `KeyBinding` com `Control` = ⌘ desde a 1.16 ([CHANGELOG#L567-L568](https://github.com/slint-ui/slint/blob/v1.18.1/CHANGELOG.md#L567-L568)). `Tooltip` desde a 1.17 ([CHANGELOG#L429](https://github.com/slint-ui/slint/blob/v1.18.1/CHANGELOG.md#L429)).
- AccessKit ligado por padrão ([winit/Cargo.toml#L64](https://github.com/slint-ui/slint/blob/v1.18.1/internal/backends/winit/Cargo.toml#L64)). Texto via parley ([core/Cargo.toml#L81](https://github.com/slint-ui/slint/blob/v1.18.1/internal/core/Cargo.toml#L81)).

**2. Desempenho e energia**
- Retido e reativo: a janela só redesenha quando muda uma propriedade lida no render ([window.rs#L562](https://github.com/slint-ui/slint/blob/v1.18.1/internal/core/window.rs#L562)).
- Parado, o loop fica em `ControlFlow::Wait` ([event_loop.rs#L223](https://github.com/slint-ui/slint/blob/v1.18.1/internal/backends/winit/event_loop.rs#L223)).
- Redesenho parcial só no renderer de software. No Skia com GPU, só com `SLINT_SKIA_PARTIAL_RENDERING` ([skia/lib.rs#L338-L347](https://github.com/slint-ui/slint/blob/v1.18.1/internal/renderers/skia/lib.rs#L338-L347)).
- Inferência: cada tick reavalia só os bindings sujos, mas a GPU pinta o frame inteiro.
- `ListView` instancia só os itens visíveis ([listview.mdx#L11-L12](https://github.com/slint-ui/slint/blob/v1.18.1/docs/astro/src/content/docs/reference/std-widgets/views/listview.mdx#L11-L12)).
- O cache de imagens é um LRU de 5 MiB ([cache.rs#L39-L53](https://github.com/slint-ui/slint/blob/v1.18.1/internal/core/graphics/image/cache.rs#L39-L53)).
- Não há blur de imagem. Há gradientes linear, radial e cônico ([CHANGELOG#L442](https://github.com/slint-ui/slint/blob/v1.18.1/CHANGELOG.md#L442)).

**3. Convivência com o engine**
- "the event loop must run in the main thread" ([lib.rs#L163-L169](https://github.com/slint-ui/slint/blob/v1.18.1/api/rs/slint/lib.rs#L163-L169)).
- A UI é acordada com `invoke_from_event_loop` ([api.rs#L1286](https://github.com/slint-ui/slint/blob/v1.18.1/internal/core/api.rs#L1286)). Não exige runtime async ([lib.rs#L286-L310](https://github.com/slint-ui/slint/blob/v1.18.1/api/rs/slint/lib.rs#L286-L310)).
- Não há `catch_unwind` nem áudio.

**4. Maturidade**
- "Slint follows a stable 1.x API" ([README#L37](https://github.com/slint-ui/slint/blob/v1.18.1/README.md#L37)). Desde a 1.15, só duas quebras marcadas ([CHANGELOG#L528](https://github.com/slint-ui/slint/blob/v1.18.1/CHANGELOG.md#L528), [#L646](https://github.com/slint-ui/slint/blob/v1.18.1/CHANGELOG.md#L646)).
- É mantido pela SixtyFPS GmbH ([Royalty-free#L7](https://github.com/slint-ui/slint/blob/v1.18.1/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md#L7)).
- Licença tríplice: GPLv3, comercial ou Royalty-free 2.0. A royalty-free exige o `AboutSlint` numa tela About ou o selo "Made with Slint" numa página pública ([Royalty-free#L21-L37](https://github.com/slint-ui/slint/blob/v1.18.1/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md#L21-L37)).
- A vitrine oficial só cita um app para macOS, um sintetizador ([madewithslint.com](https://madewithslint.com)).

**5. Visual**
- A UI é escrita na linguagem `.slint`, compilada no build, e a lógica em Rust ([README](https://github.com/slint-ui/slint/blob/v1.18.1/README.md)).
- Há `Path`, gradientes e sombras, e desenho em wgpu por baixo ou por cima via `set_rendering_notifier` ([api.rs#L521](https://github.com/slint-ui/slint/blob/v1.18.1/internal/core/api.rs#L521)).

**6. Testes**
- `i-slint-backend-testing` roda headless, com `find_by_accessible_label`, `simulate_native_drop` e tempo simulado ([testing/lib.rs#L29-L56](https://github.com/slint-ui/slint/blob/v1.18.1/internal/backends/testing/lib.rs#L29-L56)).
- É crate interno, sem semver ([README](https://github.com/slint-ui/slint/blob/v1.18.1/internal/backends/testing/README.md)).

**7. Build**
- O Skia baixa binários pré-compilados, e a doc o descreve como "Heavy disk-footprint" ([backends_and_renderers.mdx#L109](https://github.com/slint-ui/slint/blob/v1.18.1/docs/astro/src/content/docs/guide/backends-and-renderers/backends_and_renderers.mdx#L109)).
- Inferência: com FemtoVG-wgpu não há C/C++.

**Para o hog-audio:** finalista. Tem a melhor estabilidade e o melhor custo parado entre os candidatos em Rust. O preço:
- trocar o renderer padrão;
- depender de API instável para drop e titlebar até o winit 0.31;
- fazer o blur na CPU;
- cumprir a atribuição da licença.

---

## Freya 0.4.3 / 0.5.0-rc.7

0.4.0 saiu em 16/07/2026, 0.4.3 em 30/08/2026 e 0.5.0-rc.7 em 21/09/2026 ([releases](https://github.com/marc2332/freya/releases)).

Relação com o Dioxus: "Starting 0.4 Freya does no longer use Dioxus, instead it uses its own reactive core" ([README#L804-L805](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/README.md#L804-L805)). Até a 0.3 ele usava o núcleo do Dioxus. Hoje desenha com Skia e elementos próprios, sem HTML/CSS. Do Dioxus sobram o `generational-box` e, só com a feature `hotreload`, o `dioxus-devtools` ([freya-core/Cargo.toml](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya-core/Cargo.toml#L4-L17)).

**1. Integração**
- Skia com Metal no macOS ([platforms.rs#L13](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya/src/_docs/platforms.rs#L13)). Desenho próprio.
- Não tem API de menu nativo; `Menu` é um menu flutuante ([menu.rs#L53](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya-components/src/menu.rs#L53)).
- O cromo se ajusta pelo hook `with_window_attributes` do winit ([config.rs#L242-L258](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya-winit/src/config.rs#L242-L258)).
- Drop do Finder via `on_file_drop` ([exemplo](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/examples/feature_file_drop.rs#L41-L49)). Diálogo via `rfd` ([exemplo](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/examples/integration_file_picker.rs#L6-L20)).
- `Tooltip` existe ([tooltip.rs#L41](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya-components/src/tooltip.rs#L41)). AccessKit vem por padrão ([Cargo.toml#L113-L127](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya-winit/Cargo.toml#L113-L127)).

**2. Desempenho e energia**
- Redesenha a janela inteira: `canvas.clear` e "TODO: Use incremental rendering" ([render_pipeline.rs#L52-L59](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya-core/src/render_pipeline.rs#L52-L59)).
- `VirtualScrollView` renderiza só os visíveis ([virtual_scrollview.rs#L160-L169](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya-components/src/scrollviews/virtual_scrollview.rs#L160-L169)).
- Imagens são decodificadas no tamanho exibido, com cache GPU de 256 MiB ([optimizing.rs#L37-L55](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya/src/_docs/optimizing.rs#L37-L55)).
- `blur` é backdrop do Skia ([render_pipeline.rs#L146-L168](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya-core/src/render_pipeline.rs#L146-L168)).
- Inferência: com a janela inteira redesenhada, o blur é recalculado a cada tick, então convém pré-desfocar a capa.

**3. Convivência com o engine**
- Tem runtime async próprio, e o tokio é desencorajado ([tokio_integration.rs#L3-L8](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya/src/_docs/tokio_integration.rs#L3-L8)).
- `LaunchProxy::post_callback` acorda a UI ([renderer.rs#L172-L202](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya-winit/src/renderer.rs#L172-L202)).
- Em release, envolve tudo em `catch_unwind`, mostra um diálogo e faz `exit(1)` ([lib.rs#L54-L77](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya-winit/src/lib.rs#L54-L77)).
- Puxa `rodio` só com a feature `video` ([Cargo.toml#L21-L23](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/crates/freya-video/Cargo.toml#L21-L23)).

**4. Maturidade**
- Sem 1.0, e a 0.4 foi uma reescrita ([blog 0.4](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/website/src/content/blog/0.4.md#L14)). Marc Espín fez 99 dos últimos 100 commits.
- Tem 5 patrocinadores ([README#L785-L791](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/README.md#L785-L791)). Licença MIT.
- Sem fonte: app comercial no macOS.

**5 a 7**
- `canvas` com Skia e shaders SkSL ([blog 0.4](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/website/src/content/blog/0.4.md#L1017-L1048)).
- `TestingRunner` headless com snapshot ([README#L479-L518](https://github.com/marc2332/freya/blob/v0.5.0-rc.7/README.md#L479-L518)).
- O Skia vem em binários do fork pessoal do autor ([env.rs#L22-L24](https://github.com/marc2332/rust-skia/blob/5fda2d81db79d073a2f3be45d9ba7191fcfec649/skia-bindings/build_support/binary_cache/env.rs#L22-L24)). O de aarch64 tem 15,6 MB ([release 0.100.0](https://github.com/marc2332/rust-skia/releases/tag/0.100.0)).

**Para o hog-audio:** o mais completo dos desenhados fora dos finalistas, e o único com blur em GPU pronto. Fica fora pelo risco: um mantenedor, série 0.5 em rc, Skia de fork pessoal, sem menu nativo e redesenho integral.

---

## Xilem / Masonry 0.4.0

0.4.0 saiu em 29/10/2025 e não houve release em 2026. As notas dizem "alpha-quality software… making major breaking changes" ([release](https://github.com/linebender/xilem/releases/tag/v0.4.0)). O main está 294 commits à frente ([compare](https://github.com/linebender/xilem/compare/v0.4.0...main)).

- Vello sobre wgpu, com a feature `metal` no macOS ([masonry_winit/Cargo.toml#L50-L52](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry_winit/Cargo.toml#L50-L52)).
- AccessKit desde o início ([Cargo.toml#L85-L91](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/Cargo.toml#L85-L91)). Texto via parley.
- O runner ignora `DroppedFile` ([event_loop_runner.rs#L902-L937](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry_winit/src/event_loop_runner.rs#L902-L937)).
- Sem fonte: API de menu nativo.
- Guarda em cache a `Scene` de cada widget, mas tem "TODO - Handle damage regions" ([paint.rs#L95-L99](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry_core/src/passes/paint.rs#L95-L99)).
- O backend Vello em GPU rejeita filtro de blur ([scene_sink.rs#L225-L226](https://github.com/forest-rs/imaging/blob/89b364b253bd4a07732e0fb1de95ae1b313cf588/imaging_vello/src/scene_sink.rs#L225-L226)).
- O tokio multi-thread é obrigatório ([xilem/Cargo.toml#L55](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/xilem/Cargo.toml#L55)).
- Mantido pela Linebender. Em 2024, o Google Fonts financiava quatro contribuidores ([blog](https://github.com/linebender/linebender.github.io/blob/main/content/blog/2024-01-10-xilem-2024/index.md#L7)); sem fonte para 2026. Licença Apache-2.0.
- `TestHarness` roda headless com snapshot ([testing_widget.md#L15](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry/src/doc/testing_widget.md#L15)).

**Para o hog-audio:** fora. É alfa, não publica há 11 meses, não recebe drop do Finder e exige tokio.

---

## Floem 0.2.0

0.2.0 saiu em 14/11/2024. O main está 298 commits à frente e usa winit por fork git ([Cargo.toml#L65](https://github.com/lapce/floem/blob/1351ffb162faeb8be983f1301d718a0d5fb59c27/Cargo.toml#L65)). O README diz "still maturing… occasional breaking changes" ([README#L15](https://github.com/lapce/floem/blob/1351ffb162faeb8be983f1301d718a0d5fb59c27/README.md#L15)).

- wgpu com vger por padrão; vello e skia são opcionais ([README#L47](https://github.com/lapce/floem/blob/1351ffb162faeb8be983f1301d718a0d5fb59c27/README.md#L47)).
- Tem NSMenu real via muda, com aceleradores ([action.rs#L267-L277](https://github.com/lapce/floem/blob/1351ffb162faeb8be983f1301d718a0d5fb59c27/src/action.rs#L267-L277)).
- Titlebar transparente ([window/mod.rs#L287-L403](https://github.com/lapce/floem/blob/1351ffb162faeb8be983f1301d718a0d5fb59c27/src/window/mod.rs#L287-L403)), `FileDragEvent` ([dropped_file.rs#L5-L16](https://github.com/lapce/floem/blob/1351ffb162faeb8be983f1301d718a0d5fb59c27/src/event/dropped_file.rs#L5-L16)) e `open_file` sobre `rfd` ([file_action.rs#L10-L31](https://github.com/lapce/floem/blob/1351ffb162faeb8be983f1301d718a0d5fb59c27/src/platform/file_action.rs#L10-L31)).
- Não tem acessibilidade: a issue está aberta desde 14/04/2023 ([#8](https://github.com/lapce/floem/issues/8)).
- Pinta a árvore inteira a cada paint ([handle.rs#L813-L850](https://github.com/lapce/floem/blob/1351ffb162faeb8be983f1301d718a0d5fb59c27/src/window/handle.rs#L813-L850)). Parado, fica em `ControlFlow::Wait` ([app/handle.rs#L752-L786](https://github.com/lapce/floem/blob/1351ffb162faeb8be983f1301d718a0d5fb59c27/src/app/handle.rs#L752-L786)).
- Blur só em sombra ([renderer/src/lib.rs#L155-L159](https://github.com/lapce/floem/blob/1351ffb162faeb8be983f1301d718a0d5fb59c27/renderer/src/lib.rs#L155-L159)).
- O tokio é opcional ([CHANGELOG#L3-L4](https://github.com/lapce/floem/blob/1351ffb162faeb8be983f1301d718a0d5fb59c27/CHANGELOG.md#L3-L4)).
- O próprio Lapce usa uma rev git 176 commits atrás do main ([lapce Cargo.toml#L79-L89](https://github.com/lapce/lapce/blob/b604d57de4a820006d335a3be0d7583eb8fab558/Cargo.toml#L79-L89)). Licença MIT.
- `HeadlessHarness` ([headless.rs#L1-L22](https://github.com/lapce/floem/blob/1351ffb162faeb8be983f1301d718a0d5fb59c27/src/headless.rs#L1-L22)).

**Para o hog-audio:** fora. Tem a melhor integração de menu entre os desenhados, mas nenhuma acessibilidade e nenhum release em 22 meses.

---

## GPUI (Zed)

**Existe fora do repositório do Zed? Só por terceiros.**
- O código oficial vive em `zed/crates/gpui`, `gpui_platform`, `gpui_macos` e `gpui_apple`. O workspace tem `publish = false` ([Cargo.toml#L270-L271](https://github.com/zed-industries/zed/blob/v1.21.0/Cargo.toml#L270-L271)). O `gpui` sobrescreve com `publish = true`, mas continua na versão 0.2.2 ([gpui/Cargo.toml](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/Cargo.toml)). Os crates de plataforma herdam `publish = false` ([gpui_platform/Cargo.toml](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui_platform/Cargo.toml)), e é por eles que um app cria a plataforma (`gpui_platform::current_platform`, [main.rs#L86-L93](https://github.com/zed-industries/zed/blob/v1.21.0/crates/zed/src/main.rs#L86-L93)).
- O crate `gpui` do crates.io parou em 0.2.2, de 22/10/2025, antes da divisão em crates ([crates.io](https://crates.io/api/v1/crates/gpui)). `gpui_platform` não existe no crates.io.
- gpui.rs é uma landing page que aponta para o monorepo ([gpui.rs](https://www.gpui.rs/)).
- `gpui-ce` é um fork da comunidade, criado em 12/12/2025. O crate 0.2.2 saiu em 28/08/2026 e o README diz "mostly API compatible, but this is changing" ([repo](https://github.com/gpui-ce/gpui-ce)).
- `gpui-pre` 0.3.6, de 21/09/2026, é um snapshot semanal do main do Zed publicado por um desenvolvedor da Longbridge ([crates.io](https://crates.io/api/v1/crates/gpui-pre)).
- O `gpui-component` 0.6.6 fixa `gpui-pre =0.3.6` ([dependências](https://crates.io/api/v1/crates/gpui-component/0.6.6/dependencies)).
- Inferência: um app de terceiros hoje usa git dep do Zed fixada em tag, ou `gpui-pre` com versão exata.

**1. Integração**
- Metal direto no macOS, sem wgpu ([gpui_macos.rs#L18](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui_macos/src/gpui_macos.rs#L18), [README#L31](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/README.md#L31)). Desenho próprio.
- `App::set_menus` monta um NSMenu ([app.rs#L2563-L2574](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/app.rs#L2563-L2574)).
- `TitlebarOptions { appears_transparent }` ([platform.rs#L2248-L2258](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/platform.rs#L2248-L2258)) e `window_min_size` ([platform.rs#L2105](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/platform.rs#L2105)).
- `WindowBackgroundAppearance::Blurred` é um NSVisualEffectView ([window.rs#L1865-L1875](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui_macos/src/window.rs#L1865-L1875)).
- Drop do Finder via `ExternalPaths` e `on_drop` ([interactive.rs#L694](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/interactive.rs#L694)).
- `prompt_for_paths` abre o NSOpenPanel, sem filtro por tipo ([platform.rs#L2358-L2367](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/platform.rs#L2358-L2367)).
- Atalhos com `actions!` e `bind_keys` ([app.rs#L2381](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/app.rs#L2381)). O tooltip é uma view própria ([div.rs#L704-L741](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/elements/div.rs#L704-L741)).
- O AccessKit entrou em 27/05/2026 ([PR #56065](https://github.com/zed-industries/zed/pull/56065)).
- Mas o próprio Zed 1.21 roda com `new_inaccessible`, a menos que `ZED_EXPERIMENTAL_A11Y=1` ([main.rs#L86-L93](https://github.com/zed-industries/zed/blob/v1.21.0/crates/zed/src/main.rs#L86-L93)).
- Inferência: o VoiceOver existe, mas é experimental.
- Texto via Core Text ([text_system.rs#L17-L27](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui_macos/src/text_system.rs#L17-L27)).

**2. Desempenho e energia**
- "hybrid immediate and retained mode" ([README#L3](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/README.md#L3)).
- O frame só desenha se `invalidator.is_dirty()` ([window.rs#L1783-L1797](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/window.rs#L1783-L1797)). `Entity::cached` reaproveita uma subárvore ([view.rs#L224-L232](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/view.rs#L224-L232)).
- O CVDisplayLink roda enquanto a janela está visível e para quando ela fica oculta ([window.rs#L2975-L2986](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui_macos/src/window.rs#L2975-L2986)).
- Inferência: com a janela visível e parada, a main thread acorda a cada vsync (60 Hz no Air M1) sem desenhar. É o primeiro número a medir.
- Listas: `uniform_list` para altura fixa e `list` para altura variável ([uniform_list.rs#L18-L26](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/elements/uniform_list.rs#L18-L26)).
- Imagens em atlas Metal, com `ImageCache` ([image_cache.rs#L197-L222](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/elements/image_cache.rs#L197-L222)).
- `linear_gradient` existe. No GPUI do Zed, blur só em sombra ([style.rs#L355-L378](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/style.rs#L355-L378)); o gpui-ce tem `blur()` e `backdrop_blur()` ([PR #40](https://github.com/gpui-ce/gpui-ce/pull/40)).
- O único número publicado é de cerca de 4 ms por frame, num post de 2024 ([blog](https://zed.dev/blog/120fps)).

**3. Convivência com o engine**
- Exige a main thread ([platform.rs#L210](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui_macos/src/platform.rs#L210)).
- Tem executor próprio sobre GCD ([dispatcher.rs#L39-L71](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui_macos/src/dispatcher.rs#L39-L71)). O tokio é opcional, via `gpui_tokio`.
- Não há dependência de áudio.
- Há precedente: o Pawse, player bit-perfect, usa `gpui-pre` ([Cargo.toml#L34-L40](https://github.com/popovpsk/pawse/blob/fd70e9e3eeef4ec2ed731f43f7908849c9df5bd3/Cargo.toml#L34-L40)) no mesmo processo que toma hog mode com o próprio PID ([hog.rs#L207-L243](https://github.com/popovpsk/pawse/blob/fd70e9e3eeef4ec2ed731f43f7908849c9df5bd3/crates/audio_output/src/exclusive/macos/hog.rs#L207-L243)). O projeto tem 18 estrelas.
- Ressalva, verificada no clone local em 26/09: no Pawse o hog é opcional. O app sempre abre em modo compartilhado via cpal (`audio_output/src/output.rs:112`). O botão de exclusivo fica escondido por padrão (`show_hog_button: false`, `pawse/src/settings_store.rs:388`), e a escolha não é persistida. O precedente prova que GPUI e hog convivem no mesmo processo; não prova um player bit-perfect por padrão.
- Inferência: os callbacks do AppKit são `extern "C"` sem `catch_unwind`, então um pânico aborta.

**4. Maturidade**
- Pré-1.0: "There will often be breaking changes between versions" ([README#L8](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/README.md#L8)).
- Mantido pela Zed Industries, que levantou uma Series B de US$32M em 20/08/2025 ([blog](https://zed.dev/blog/sequoia-backs-zed)). Licença Apache-2.0.
- Em produção no Zed e no Longbridge Pro ([gpui-kit README#L37](https://github.com/longbridge/gpui-kit/blob/452e72f24a183174551fdec3db570997b1e1fc8f/README.md#L37)). Há vários players na lista oficial ([awesome-gpui#L94-L104](https://github.com/zed-industries/awesome-gpui/blob/f8b23093aee1d9584316d950b42f5a3c030f6237/README.md#L94-L104)).

**5. Visual**
- `canvas` com `paint_quad`, `paint_path` e `paint_image` ([canvas.rs#L10](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/elements/canvas.rs#L10)).
- Não há shader próprio; os temas ficam no app ou no gpui-component.

**6. Testes**
- `#[gpui::test]` com `TestAppContext`, `simulate_keystrokes`, `advance_clock` e `VisualTestContext` ([test_context.rs#L21](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/src/app/test_context.rs#L21)).

**7. Build**
- Exige o Xcode completo para compilar os shaders Metal ([README#L54-L71](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui/README.md#L54-L71), [build.rs#L141-L172](https://github.com/zed-industries/zed/blob/v1.21.0/crates/gpui_apple/build.rs#L141-L172)), a menos que se use a feature `runtime_shaders`.
- Como git dep, o monorepo tem cerca de 527 MB (API do GitHub).

**Para o hog-audio:** finalista e recomendação. Tem a melhor integração com o macOS entre os candidatos em Rust e um precedente real de hog mode no mesmo processo. Os riscos:
- distribuição, porque não há crate oficial atual;
- API instável;
- acessibilidade experimental;
- display link acordando a cada vsync.

---

## Makepad

O crates.io parou em 1.0.0, de 13/05/2025 ([crates.io](https://crates.io/api/v1/crates/makepad-widgets)). O branch `dev` está em 2.0.0, não publicado, trocando o DSL `live_design!` pelo "Splash" ([splash.md](https://github.com/makepad/makepad/blob/229057962aa45872c0abd605a0aa7f75c68ddde3/splash.md)).

- Metal próprio ([metal.rs#L3692](https://github.com/makepad/makepad/blob/229057962aa45872c0abd605a0aa7f75c68ddde3/platform/src/os/apple/metal.rs#L3692)).
- NSMenu via `MacosMenu` ([macos_menu.rs#L12-L28](https://github.com/makepad/makepad/blob/229057962aa45872c0abd605a0aa7f75c68ddde3/platform/src/macos_menu.rs#L12-L28)).
- NSOpenPanel com filtros ([file_dialogs.rs#L176-L188](https://github.com/makepad/makepad/blob/229057962aa45872c0abd605a0aa7f75c68ddde3/platform/src/file_dialogs.rs#L176-L188)). Drop do Finder funciona.
- No macOS, a acessibilidade é descartada: `CxOsOp::AccessibilityUpdate(_) => {}` ([macos.rs#L2029](https://github.com/makepad/makepad/blob/229057962aa45872c0abd605a0aa7f75c68ddde3/platform/src/os/apple/macos/macos.rs#L2029)). A issue está aberta desde 2023 ([#196](https://github.com/makepad/makepad/issues/196)).
- Texto próprio (rustybuzz e SDF), sem Core Text ([draw/Cargo.toml](https://github.com/makepad/makepad/blob/229057962aa45872c0abd605a0aa7f75c68ddde3/draw/Cargo.toml#L18-L33)).
- Sem fonte: API de tamanho mínimo de janela.
- Dorme 0,2 s depois do último trabalho ([macos.rs#L1205-L1215](https://github.com/makepad/makepad/blob/229057962aa45872c0abd605a0aa7f75c68ddde3/platform/src/os/apple/macos/macos.rs#L1205-L1215)).
- `PortalList` virtualiza a lista, e há blur gaussiano de backdrop em GPU ([gauss_view.rs#L253-L295](https://github.com/makepad/makepad/blob/229057962aa45872c0abd605a0aa7f75c68ddde3/widgets/src/gauss_view.rs#L253-L295)).
- Tem API de áudio sobre AudioUnit, mas o device só é aberto se o app chamar ([apple_media.rs#L60-L65](https://github.com/makepad/makepad/blob/229057962aa45872c0abd605a0aa7f75c68ddde3/platform/src/os/apple/apple_media.rs#L60-L65)).
- Usa `catch_unwind` nas fronteiras ObjC do timer ([macos_delegates.rs#L63-L90](https://github.com/makepad/makepad/blob/229057962aa45872c0abd605a0aa7f75c68ddde3/platform/src/os/apple/macos/macos_delegates.rs#L63-L90)).
- O Robrix o usa via git, no branch `dev` ([Cargo.toml#L17](https://github.com/project-robius/robrix/blob/4b61f7fcc4fe71c22b5d33019402c57dbbfdb42c/Cargo.toml#L17)).
- Sem fonte: financiamento da Makepad B.V.

**Para o hog-audio:** fora. Não tem VoiceOver no macOS, e a API 2.0 não está publicada, no meio de uma troca de DSL.

---

## Vizia 0.4.0

0.4.0 saiu em 23/04/2026 e 0.3.0 em 16/04/2025 ([releases](https://github.com/vizia/vizia/releases/tag/0.4.0)). Licença MIT. Um mantenedor domina: 1.559 commits contra 111 do segundo ([contributors](https://api.github.com/repos/vizia/vizia/contributors)).

- Renderiza com Skia Ganesh sobre OpenGL via glutin ([window.rs#L110-L119](https://github.com/vizia/vizia/blob/0.4.0/crates/vizia_winit/src/window.rs#L110-L119), [ARCHITECTURE.md#L40](https://github.com/vizia/vizia/blob/0.4.0/ARCHITECTURE.md#L40)).
- O Skia suporta Metal, e o Vizia até compila a feature `metal` do skia-safe no macOS ([Cargo.toml#L101-L103](https://github.com/vizia/vizia/blob/0.4.0/Cargo.toml#L101-L103)). Mas os dois backends de janela só criam contexto GL: `direct_contexts::make_gl` no winit ([window.rs#L152-L164](https://github.com/vizia/vizia/blob/0.4.0/crates/vizia_winit/src/window.rs#L152-L164)) e no baseview ([window.rs#L59-L71](https://github.com/vizia/vizia/blob/0.4.0/crates/vizia_baseview/src/window.rs#L59-L71)). O `main` em `426d2e7` (21/08/2026) continua igual.
- O Metal está a caminho: a PR [#704](https://github.com/vizia/vizia/pull/704), do mantenedor principal, adiciona backends Metal, DX12 e Vulkan, com Metal como padrão no macOS (`draw_surface/metal.rs`). Ela está aberta desde 14/05/2026, sem atividade desde 23/05, com conflito contra o `main` e sem revisão humana.
- O `MenuBar` é desenhado dentro da janela ([menu.rs#L53-L57](https://github.com/vizia/vizia/blob/0.4.0/crates/vizia_core/src/views/menu.rs#L53-L57)). Não há diálogo nativo.
- O drop entra como `DropData::File`, e o `HoveredFile` é ignorado ([application.rs#L585-L590](https://github.com/vizia/vizia/blob/0.4.0/crates/vizia_winit/src/application.rs#L585-L590)).
- AccessKit 0.24 ([Cargo.toml#L57-L58](https://github.com/vizia/vizia/blob/0.4.0/Cargo.toml#L57-L58)).
- Usa dirty rects ([draw.rs#L25-L60](https://github.com/vizia/vizia/blob/0.4.0/crates/vizia_core/src/systems/draw.rs#L25-L60)), mas elementos com `backdrop-filter` entram em todo dirty rect ([draw.rs#L74-L108](https://github.com/vizia/vizia/blob/0.4.0/crates/vizia_core/src/systems/draw.rs#L74-L108)).
- Na 0.4.0 o layout refaz a árvore inteira ([layout.rs#L9-L14](https://github.com/vizia/vizia/blob/0.4.0/crates/vizia_core/src/systems/layout.rs#L9-L14)).
- `VirtualList` ([virtual_list.rs#L8-L37](https://github.com/vizia/vizia/blob/0.4.0/crates/vizia_core/src/views/virtual_list.rs#L8-L37)). `filter: blur()` existe.
- A UI é acordada via `ContextProxy` ([proxy.rs#L45-L56](https://github.com/vizia/vizia/blob/0.4.0/crates/vizia_core/src/context/proxy.rs#L45-L56)). Sem tokio.
- A troca de lenses por signals quebrou a API na 0.4 ([MIGRATION](https://github.com/vizia/vizia/blob/0.4.0/MIGRATION_LENS_TO_SIGNALS.md)).
- Há um jitter no resize do macOS aberto desde 2024 ([#456](https://github.com/vizia/vizia/issues/456)).
- Sem fonte: app desktop em produção no macOS.

**Para o hog-audio:** fora. Hoje renderiza por OpenGL; o Metal só existe na PR #704, parada e com conflito. Mesmo com ela mergeada, continuam os outros motivos: não tem menu nem diálogo nativos, depende de um mantenedor, refaz o layout da árvore inteira e tem o jitter de resize no macOS.

---

## AppKit direto do Rust via objc2-app-kit

objc2 0.6.4 saiu em 26/02/2026 e objc2-app-kit 0.3.2 em 04/10/2025 ([crates.io](https://crates.io/api/v1/crates/objc2-app-kit)). Nenhum dos dois chegou à 1.0.

**1. Integração**
- É o AppKit real. Há uma feature por header, com NSTableView, NSVisualEffectView, NSMenu e NSOpenPanel ([features](https://docs.rs/crate/objc2-app-kit/0.3.2/features)).
- Subclasses e delegates se declaram com `define_class!` ([docs.rs](https://docs.rs/objc2/0.6.4/objc2/macro.define_class.html)).
- A acessibilidade vem do próprio AppKit ([Apple](https://developer.apple.com/documentation/appkit/accessibility-for-appkit)).
- Parte da API segue `unsafe`, como `NSTableView::setDataSource` ([docs.rs](https://docs.rs/objc2-app-kit/0.3.2/objc2_app_kit/struct.NSTableView.html)).

**2. Desempenho e energia**
- `NSTableView` reusa as views das linhas ([Apple](https://developer.apple.com/documentation/appkit/nstableview/makeview(withidentifier:owner:))).
- Blur com `NSVisualEffectView` ([Apple](https://developer.apple.com/documentation/appkit/nsvisualeffectview)) ou `CIGaussianBlur` ([Apple](https://developer.apple.com/documentation/coreimage/cigaussianblur)).
- Inferência: o teto é o mesmo do SwiftUI, mas sem diff de views; tudo é escrito à mão.

**3. Convivência com o engine**
- Uma linguagem só, sem FFI.
- O trampolim do `dispatch2` é `extern "C"`, então um pânico ali aborta ([utils.rs#L4-L12](https://github.com/madsmtm/objc2/blob/dispatch2-0.3.1/crates/dispatch2/src/utils.rs#L4-L12)).
- Os métodos de `define_class!` são `extern "C-unwind"`, e o unwind através de frames ObjC "works in practice", sem garantia especificada ([exception.rs#L272-L277](https://github.com/madsmtm/objc2/blob/objc2-0.6.4/crates/objc2/src/exception.rs#L272-L277)).

**4. Maturidade**
- Tem mantenedor quase único, com cerca de 397 dos 406 commits do último ano (API de estatísticas do GitHub).
- O main tem uma seção Unreleased com mudanças BREAKING ([CHANGELOG#L7-L123](https://github.com/madsmtm/objc2/blob/b113b0484100b68726b546dc35814087e5da430b/crates/objc2/CHANGELOG.md#L7-L123)).
- Bug de corretude: use-after-free em release, alcançável por código safe como `NSWindow::contentView`, com objc2 0.6.4 ([#861](https://github.com/madsmtm/objc2/issues/861)). Foi corrigido no main em 25/09/2026 ([PR #862](https://github.com/madsmtm/objc2/pull/862)), ainda sem release.
- O Zed usa objc2 0.6 ([Cargo.toml#L698](https://github.com/zed-industries/zed/blob/v1.21.0/Cargo.toml#L698)). O winit 0.30.13 usa o 0.5.2 ([Cargo.toml#L122](https://github.com/rust-windowing/winit/blob/v0.30.13/Cargo.toml#L122)).
- Inferência: o GPUI está exposto ao #861 até sair release.
- Sem fonte: se o bug existe no 0.5.2.
- Usado por winit, wgpu, Zed e tao, para janela e eventos.
- Sem fonte: app em produção com UI de widgets AppKit escrita em objc2.
- O `cacao`, única camada de mais alto nível, está parado em 0.4.0-beta2, de 2023 ([Cargo.toml](https://github.com/ryanmcgrath/cacao/blob/94ab81584082a4be46c82da991167424ba2ff9cf/Cargo.toml#L23-L28)).

**6 e 7**
- XCUITest funciona em qualquer app AppKit ([Apple](https://developer.apple.com/documentation/xcuiautomation/xcuiapplication/init(bundleidentifier:))). O estado se testa com `cargo test`.
- Só exige o SDK da Apple.

**Para o hog-audio:** é o único caminho "nativo e só Rust", com o mesmo teto do SwiftUI. Mas é escrito à mão, com `unsafe`, sobre bindings 0.x de um mantenedor e com um bug de memória no release atual. Não serve como desenho definitivo agora.

---

## cxx-qt 0.10.0 (achado na varredura)

0.10.0 saiu em 24/08/2026. Mantido pela KDAB, licença MIT OR Apache-2.0. O README diz "early development and the API changes frequently" ([README#L49](https://github.com/KDAB/cxx-qt/blob/v0.10.0/README.md#L49)).

- O Qt Quick renderiza via RHI, com Metal no macOS ([doc Qt](https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph-renderer.html)).
- `MenuBar` nativo desde o Qt 6.8 ([doc](https://doc.qt.io/qt-6/qml-qtquick-controls-menubar.html)) e `FileDialog` nativo ([doc](https://doc.qt.io/qt-6/qml-qtquick-dialogs-filedialog.html)).
- Tem acessibilidade no macOS ([doc](https://doc.qt.io/qt-6/qaccessible.html)).
- A UI fica em QML, não em Rust, e o Qt em C++ entra no processo do engine.
- O cxx aborta em pânico ([unwind.rs#L5-L21](https://github.com/dtolnay/cxx/blob/b2971717da380618ea8ab5a3bf9ee68434e1f684/src/unwind.rs#L5-L21)).
- O Qt é LGPLv3 ou comercial ([doc](https://doc.qt.io/qt-6/licensing.html)).

**Para o hog-audio:** não cumpre "UI inteira em Rust"; troca Swift por QML.

## wxDragon 0.9.22 (achado na varredura)

0.9.22 saiu em 17/09/2026 ([release](https://github.com/AllenDang/wxDragon/releases/tag/v0.9.22)).

- Controles Cocoa nativos via wxWidgets ([README#L145](https://github.com/AllenDang/wxDragon/blob/v0.9.22/README.md#L145)).
- A própria 0.9.22 corrige double free e corrupção de heap ([CHANGELOG#L3-L24](https://github.com/AllenDang/wxDragon/blob/v0.9.22/CHANGELOG.md#L3-L24)).
- Compila o wxWidgets com CMake ([wxdragon-sys README#L17-L23](https://github.com/AllenDang/wxDragon/blob/v0.9.22/rust/wxdragon-sys/README.md#L17-L23)).

**Para o hog-audio:** fora. A camada C++ ainda tem bugs de memória, e a liberdade visual não comporta a direção atual.

---

## Descartados

- gtk4-rs e Relm4: no macOS o GTK usa OpenGL, não empacota .app e a acessibilidade é opt-in ([docs.gtk.org](https://docs.gtk.org/gtk4/osx.html)).
- fltk-rs 1.5.23: aparência não nativa e acessibilidade num crate separado ([README#L31](https://github.com/fltk-rs/fltk-rs/blob/1.5.23/README.md#L31)).
- flutter_rust_bridge 2.13.0 e rinf 8.10.1: a UI fica em Dart ([crates.io](https://crates.io/api/v1/crates/flutter_rust_bridge)).
- cushy 0.4.0, de 20/08/2024: sem release em 2026 ([crates.io](https://crates.io/api/v1/crates/cushy)).
- kas 0.17.0, de 27/01/2026: nenhum app desktop real encontrado ([README](https://github.com/kas-gui/kas/blob/52d5162bf6b8374a99844114da46333b8cb02d14/README.md#L103-L112)).
- Ribir 0.4.0-alpha.65: o README diz que os widgets não estão prontos para produção ([repo](https://github.com/RibirX/Ribir)).
- tessera-ui: o último release real é o 2.5.0, de 18/11/2025 ([crates.io](https://crates.io/api/v1/crates/tessera-ui)).
- wgpui 0.3.6: fork do gpui-ce com 145 downloads ([repo](https://github.com/Muktidaya/wgpui)).
- Zng 0.24.5: WebRender sobre OpenGL ([crates.io](https://crates.io/api/v1/crates/zng-view)).
- Azul 0.2.0: "Azul is NOT usable yet" ([README](https://github.com/fschutt/azul/blob/626aa43c6ebecd61e1e720dbf9eadeb42b2d7d86/README.md)).
- cacao: parado desde 2023 ([crates.io](https://crates.io/api/v1/crates/cacao)).

---

## O que um protótipo precisa medir

Finalistas: GPUI e Slint. O app SwiftUI atual serve de controle, depois de trocar `ObservableObject` por `@Observable` e `id: \.offset` por um ID estável, para a comparação ser justa.

Montagem:
- a mesma tela nos três: duas colunas, fundo com a capa pré-desfocada, fila de 2.000 faixas com miniatura de 44 pt, relógio de progresso;
- build release no Air M1, na bateria, com brilho fixo e janela de 1280×800;
- o mesmo diretório de fixtures em todos;
- GPUI via `gpui-pre` com versão exata;
- Slint com `renderer-femtovg-wgpu` e depois `renderer-skia`, para ver o custo de cada um.

**1. Custo parado, com a janela visível e o player pausado, por 60 s**
- Comando: `sudo powermetrics --samplers tasks --show-process-energy -i 1000 -n 60`.
- Ler CPU ms/s, "interrupt wakeups" e "short timers" (definidos em `man powermetrics`) e o energy impact.
- Hipótese a derrubar ou confirmar: o display link do GPUI acordando a 60 Hz.

**2. Tocando, com o relógio a 4 Hz e depois a 10 Hz**
- As mesmas métricas, mais `--samplers tasks,gpu_power`.
- Critério: nenhum candidato pode passar o controle SwiftUI em energy impact.

**3. Rolagem da fila inteira com miniaturas**
- Instruments via `xcrun xctrace record --template 'Animation Hitches' --attach <pid>` e `--template 'Metal System Trace'` (os dois templates existem no Xcode instalado).
- Medir hitches e o p99 do tempo de frame.
- Depois de rolar tudo, medir a memória com `footprint <pid>`.

**4. Áudio sob estresse de UI**
- Tocar uma faixa 24/192 enquanto rola a fila, redimensiona a janela e troca de faixa, por 5 minutos.
- Critério: zero underruns (`status.underruns()`, `rust/src/playback.rs:90`).
- Critério: zero notificações de `kAudioDeviceProcessorOverload`, que avisa quando "an IO cycle has run past its deadline" (`CoreAudio.framework/Headers/AudioHardware.h:921-925` no SDK do Xcode).
- O template `Audio System Trace` do Instruments mostra o ciclo do IOProc.

**5. Pânico na UI**
- Injetar `panic!` num handler de clique.
- Verificar se o processo aborta ou desenrola.
- Verificar se `kAudioDevicePropertyHogMode` volta a -1 e se o device sai do rate da última faixa, que é o risco citado em `apps/player/Sources/HogPlayer/HogPlayerApp.swift:47-50`.

**6. VoiceOver**
- Percorrer a fila e confirmar que título, artista, duração e selo são lidos.
- Acionar Delete e ⌘O pelo teclado.
- Conferir com o Accessibility Inspector.
- No GPUI, testar com a acessibilidade ligada (o padrão de `gpui_platform::application()`).

**7. Integração, como checklist sim ou não**
- ⌘O aparece no menu.
- Drop de vários arquivos de uma vez.
- Titlebar transparente.
- Tamanho mínimo da janela.
- Tooltip no selo.
- Texto comparado por captura a 2x contra o SwiftUI.

**8. Build**
- Tempo de build release limpo, tamanho do binário e `cargo tree --prefix none | sort -u | wc -l`.
- Se o build exige o Xcode completo.

Regra de decisão sugerida:
- O GPUI fica se o custo parado não passar o do controle SwiftUI e se o VoiceOver ler a fila.
- Se falhar em qualquer um dos dois, o Slint fica.
- Se os dois falharem na energia ou no VoiceOver, o SwiftUI continua.

---

## Fontes

Todas foram abertas via `gh api`, API do crates.io, WebFetch ou leitura local. Os links inline acima são a referência exata; aqui vão as páginas-base.

Locais:
- `~/work/learning-playground/rust-playground/referencias/fastpotify-ui-egui.md`
- `~/work/audio-toolbox/spotifast/Cargo.toml`
- `apps/player/Package.swift`, `apps/player/Sources/**`, `apps/player/Tests/**`, `Makefile`, `rust/src/playback.rs`
- `$(xcrun --show-sdk-path)/System/Library/Frameworks/CoreAudio.framework/Headers/AudioHardware.h`
- `man powermetrics`, `man top`, `man footprint`, `xcrun xctrace list templates`

Apple:
- https://developer.apple.com/library/archive/documentation/GraphicsImaging/Conceptual/OpenGL-MacProgGuide/opengl_intro/opengl_intro.html
- https://developer.apple.com/documentation/macos-release-notes/macos-27-release-notes
- https://developer.apple.com/documentation/swiftui/nshostingview
- https://developer.apple.com/documentation/swiftui/commandmenu
- https://developer.apple.com/documentation/swiftui/view/help(_:)
- https://developer.apple.com/documentation/swiftui/view/ondeletecommand(perform:)
- https://developer.apple.com/documentation/swiftui/scene/windowresizability(_:)
- https://developer.apple.com/documentation/swiftui/view/dropdestination(for:action:istargeted:)
- https://developer.apple.com/documentation/swiftui/view/fileimporter(ispresented:allowedcontenttypes:allowsmultipleselection:oncompletion:)
- https://developer.apple.com/documentation/swiftui/accessibility-fundamentals
- https://developer.apple.com/documentation/swiftui/nsviewrepresentable
- https://developer.apple.com/documentation/swiftui/migrating-from-the-observable-object-protocol-to-the-observable-macro
- https://developer.apple.com/documentation/swiftui/lazyvstack
- https://developer.apple.com/documentation/swiftui/material
- https://developer.apple.com/documentation/swiftui/view
- https://developer.apple.com/documentation/swiftui/shaderlibrary
- https://developer.apple.com/documentation/swiftui/view/layereffect(_:maxsampleoffset:isenabled:)
- https://developer.apple.com/documentation/foundation/timer/tolerance
- https://developer.apple.com/documentation/testing
- https://developer.apple.com/documentation/xcuiautomation
- https://developer.apple.com/documentation/xcuiautomation/xcuiapplication/init(bundleidentifier:)
- https://developer.apple.com/documentation/appkit/accessibility-for-appkit
- https://developer.apple.com/documentation/appkit/nstableview/makeview(withidentifier:owner:)
- https://developer.apple.com/documentation/appkit/nsvisualeffectview
- https://developer.apple.com/documentation/coreimage/cigaussianblur
- https://developer.apple.com/documentation/coreaudio/kaudiodeviceprocessoroverload
- https://developer.apple.com/videos/play/wwdc2023/10160/
- https://developer.apple.com/videos/play/wwdc2026/269/

Rust e bibliotecas comuns:
- https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/
- https://github.com/rust-windowing/winit/tree/v0.30.13
- https://github.com/AccessKit/accesskit
- https://github.com/mozilla/uniffi-rs/tree/v0.32.0
- https://github.com/tauri-apps/muda
- https://github.com/PolyMeilex/rfd/tree/0.17.2

Por candidato:
- egui: https://github.com/emilk/egui/tree/0.36.2, PRs e issues #8398, #8577, #8631, #7059, #7761, #2647, #1016, https://github.com/rerun-io/rerun/releases/tag/0.38.1, https://crates.io/api/v1/crates/eframe
- Iced: https://github.com/iced-rs/iced/tree/0.14.0, master em `10e9b99`, #552, #3111, #2662, #3429, https://github.com/squidowl/halloy
- Slint: https://github.com/slint-ui/slint/tree/v1.18.1, PR #12294, https://madewithslint.com
- Freya: https://github.com/marc2332/freya/tree/v0.5.0-rc.7, https://github.com/marc2332/freya/tree/v0.4.3, https://github.com/marc2332/rust-skia/releases/tag/0.100.0
- Xilem: https://github.com/linebender/xilem (v0.4.0 e main em `271a27a`), https://github.com/forest-rs/imaging, blog da Linebender
- Floem: https://github.com/lapce/floem (main em `1351ffb`), #8, https://github.com/lapce/lapce
- GPUI: https://github.com/zed-industries/zed/tree/v1.21.0, PRs #56065, #57954, #64143, https://www.gpui.rs/, https://zed.dev/blog/120fps, https://zed.dev/blog/sequoia-backs-zed, https://github.com/gpui-ce/gpui-ce, https://crates.io/api/v1/crates/gpui-pre, https://github.com/longbridge/gpui-kit, https://github.com/zed-industries/awesome-gpui, https://github.com/popovpsk/pawse
- Makepad: https://github.com/makepad/makepad (dev em `2290579`), #196, https://github.com/project-robius/robrix
- Vizia: https://github.com/vizia/vizia/tree/0.4.0, #456, #671, PR #744
- objc2: https://github.com/madsmtm/objc2, #861, PR #862, https://docs.rs/objc2-app-kit/0.3.2, https://docs.rs/objc2/0.6.4, https://github.com/ryanmcgrath/cacao
- cxx-qt e Qt: https://github.com/KDAB/cxx-qt/tree/v0.10.0, https://doc.qt.io/qt-6/, https://github.com/dtolnay/cxx
- wxDragon: https://github.com/AllenDang/wxDragon/tree/v0.9.22
- Varredura: crates.io de gtk4, relm4, fltk, flutter_rust_bridge, rinf, cushy, kas, ribir, tessera-ui, wgpui, zng-view, azul e cacao; https://docs.gtk.org/gtk4/osx.html
