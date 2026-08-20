# Orquestra o core Rust, o app Swift e as provas de bit-perfect.
RUST_BIN  := rust/target/release/hog-audio
FFI_DIR   := apps/player/Sources/HogAudioFFI
BIND_DIR  := apps/player/Sources/HogAudioBindings
APP_DIR   := apps/player/HogAudio.app

.PHONY: all rust fmt fmt-check lint rust-test hw-test bindings swift-test test fixtures \
        icon verify verify-pause verify-volume info play app run-app clean

all: rust

rust:
	@cd rust && cargo build --release

# Formatação e clippy são gate, não sugestão. O `unsafe` do HAL e do IOProc é onde moram os
# defeitos caros deste projeto, e o clippy é a única ferramenta que os lê antes do usuário
# ouvir o resultado — `not_unsafe_ptr_arg_deref` existe exatamente para esta classe de código.
fmt:
	@cd rust && cargo fmt

fmt-check:
	@cd rust && cargo fmt --check

lint:
	@cd rust && cargo clippy --all-targets -- -D warnings

rust-test:
	@cd rust && cargo test

# Os testes de hardware tomam o device de saída de verdade. Em paralelo eles disputam o mesmo
# device e reprovam sem que haja defeito, então rodam em série.
hw-test:
	@cd rust && cargo test -- --ignored --test-threads=1

# Os bindings são gerados a partir da staticlib, então o .app não precisa embarcar dylib.
bindings: rust
	@mkdir -p $(FFI_DIR)/include $(BIND_DIR)
	@cd rust && cargo run -q --release --bin uniffi-bindgen -- \
	  generate --library target/release/libhog_audio.a \
	  --language swift --out-dir /tmp/hog_bindings
	@cp /tmp/hog_bindings/hog_audioFFI.h $(FFI_DIR)/include/
	@cp /tmp/hog_bindings/hog_audioFFI.modulemap $(FFI_DIR)/include/module.modulemap
	@cp /tmp/hog_bindings/hog_audio.swift $(BIND_DIR)/
	@echo '// um alvo C do SPM exige ao menos um arquivo-fonte' > $(FFI_DIR)/empty.c
	@echo "bindings gerados em $(FFI_DIR) e $(BIND_DIR)"

swift-test: bindings
	@cd apps/player && swift test

test: fmt-check lint rust-test swift-test

# Gera os arquivos de áudio que os testes consomem. Áudio não é versionado neste repo, então
# um clone limpo precisa rodar isto uma vez antes de `make test` — sem as fixtures os testes
# que dependem delas falham, de propósito, em vez de passar sem exercitar nada.
fixtures:
	@./tools/make_fixtures.sh

# make verify FILE=testdata/t96_24.flac BITS=24
# Compara os bytes que o player entregaria ao IOProc com o PCM que o ffmpeg extrai do mesmo
# arquivo. O ffmpeg é o oráculo: é ele que torna a prova independente deste código.
verify: rust
	@test -n "$(FILE)" || { echo "uso: make verify FILE=arquivo.flac BITS=24"; exit 2; }
	@ffmpeg -v error -y -i "$(FILE)" -f s$(BITS)le /tmp/hog_ref.raw
	@./$(RUST_BIN) --dump /tmp/hog_rust.raw "$(FILE)"
	@python3 tools/verify_bitperfect.py /tmp/hog_ref.raw /tmp/hog_rust.raw --bits $(BITS)

# make verify-pause FILE=testdata/t96_24.flac
# O pause não pode descartar nem duplicar bytes do ring buffer: o dump com uma pausa
# injetada no meio tem de sair idêntico ao dump sem pausa.
verify-pause: rust
	@test -n "$(FILE)" || { echo "uso: make verify-pause FILE=arquivo.flac"; exit 2; }
	@./$(RUST_BIN) --dump /tmp/hog_sem_pausa.raw "$(FILE)" >/dev/null
	@./$(RUST_BIN) --dump /tmp/hog_com_pausa.raw --pause-at 100000 "$(FILE)" >/dev/null
	@test -s /tmp/hog_sem_pausa.raw || { echo "FALHOU: dump vazio"; exit 1; }
	@test -s /tmp/hog_com_pausa.raw || { echo "FALHOU: dump vazio"; exit 1; }
	@cmp /tmp/hog_sem_pausa.raw /tmp/hog_com_pausa.raw \
	  && echo "pause: fluxo idêntico com e sem pausa"

# make verify-volume FILE=testdata/t96_24.flac
# O volume é aplicado no device, nunca nas amostras que o dump grava. O modo dump adquire o
# device e aplica o volume de verdade, mas não registra o AudioDeviceIOProc: um ganho aplicado
# dentro do callback de reprodução passaria despercebido por este teste (ver README).
verify-volume: rust
	@test -n "$(FILE)" || { echo "uso: make verify-volume FILE=arquivo.flac"; exit 2; }
	@./$(RUST_BIN) --dump /tmp/hog_vol20.raw --volume 20 "$(FILE)" >/dev/null
	@./$(RUST_BIN) --dump /tmp/hog_vol90.raw --volume 90 "$(FILE)" >/dev/null
	@test -s /tmp/hog_vol20.raw || { echo "FALHOU: dump vazio"; exit 1; }
	@test -s /tmp/hog_vol90.raw || { echo "FALHOU: dump vazio"; exit 1; }
	@cmp /tmp/hog_vol20.raw /tmp/hog_vol90.raw \
	  && echo "volume: amostras idênticas a 20% e 90%"

# make info FILE=musicas/faixa.flac — mostra o que seria negociado, sem tocar no device.
info: rust
	@test -n "$(FILE)" || { echo "uso: make info FILE=caminho/do/arquivo.flac"; exit 2; }
	@./$(RUST_BIN) --info "$(FILE)"

# make play FILE=musicas/faixa.flac
play: rust
	@test -n "$(FILE)" || { echo "uso: make play FILE=caminho/do/arquivo.flac"; exit 2; }
	@./$(RUST_BIN) "$(FILE)"

# Monta o .app: sem Info.plist o macOS trata o binário como processo de segundo plano e
# ele nunca abre janela nem recebe foco de teclado.
app: bindings
	@cd apps/player && swift build -c release
	@mkdir -p $(APP_DIR)/Contents/MacOS $(APP_DIR)/Contents/Resources
	@cp apps/player/Resources/Info.plist $(APP_DIR)/Contents/Info.plist
	@cp apps/player/Resources/AppIcon.icns $(APP_DIR)/Contents/Resources/
	@cp apps/player/.build/release/HogPlayer $(APP_DIR)/Contents/MacOS/HogAudio
	@echo "app montado em $(APP_DIR)"

# O .icns é versionado; só rode isto depois de mexer no SVG. Precisa de rsvg-convert.
icon:
	@./tools/make_icon.sh

# make run-app FILE=musicas/faixa.flac
run-app: app
	@test -n "$(FILE)" || { echo "uso: make run-app FILE=caminho/do/arquivo.flac"; exit 2; }
	@$(APP_DIR)/Contents/MacOS/HogAudio "$(FILE)"

clean:
	@rm -rf rust/target apps/player/.build $(APP_DIR)
