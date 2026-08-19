# Orquestra as duas implementações e a comparação entre elas.
CPP_BUILD := cpp/build
RUST_BIN  := rust/target/release/hog-audio
RUST_LIB  := rust/target/release/libhog_audio.a
FFI_DIR   := apps/player/Sources/HogAudioFFI
BIND_DIR  := apps/player/Sources/HogAudioBindings
APP_DIR   := apps/player/HogAudio.app

.PHONY: all cpp cpp-test cpp-asan rust rust-test rust-lib bindings swift-test test verify verify-pause verify-volume info play app run-app clean

all: cpp rust

cpp:
	@cmake -B $(CPP_BUILD) -S cpp -DCMAKE_BUILD_TYPE=Release >/dev/null
	@MAKEFLAGS= cmake --build $(CPP_BUILD)

cpp-test: cpp
	@ctest --test-dir $(CPP_BUILD) --output-on-failure

# Os mesmos testes sob AddressSanitizer e UBSan: o ring buffer roda em thread de tempo
# real, e um transbordo ali corrompe memória sem que os asserts percebam.
cpp-asan:
	@cmake -B cpp/build-asan -S cpp -DCMAKE_BUILD_TYPE=Debug -DHOG_SANITIZE=ON >/dev/null
	@MAKEFLAGS= cmake --build cpp/build-asan
	@ctest --test-dir cpp/build-asan --output-on-failure

rust:
	@cd rust && cargo build --release

rust-test:
	@cd rust && cargo test

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

swift-test: bindings
	@cd apps/player && swift test

test: cpp-test rust-test swift-test

# make verify FILE=testdata/t96_24.flac BITS=24
# Compara os bytes que cada implementação entregaria ao IOProc com o PCM que o ffmpeg
# extrai do mesmo arquivo. As três comparações precisam bater.
verify: all
	@test -n "$(FILE)" || { echo "uso: make verify FILE=arquivo.flac BITS=24"; exit 2; }
	@ffmpeg -v error -y -i "$(FILE)" -f s$(BITS)le /tmp/hog_ref.raw
	@./$(CPP_BUILD)/hog-audio --dump /tmp/hog_cpp.raw "$(FILE)"
	@./$(RUST_BIN) --dump /tmp/hog_rust.raw "$(FILE)"
	@echo "--- C++ contra o oraculo ffmpeg ---"
	@python3 tools/verify_bitperfect.py /tmp/hog_ref.raw /tmp/hog_cpp.raw --bits $(BITS)
	@echo "--- Rust contra o oraculo ffmpeg ---"
	@python3 tools/verify_bitperfect.py /tmp/hog_ref.raw /tmp/hog_rust.raw --bits $(BITS)
	@echo "--- C++ contra Rust ---"
	@cmp /tmp/hog_cpp.raw /tmp/hog_rust.raw && echo "dumps identicos"

# make verify-pause FILE=testdata/t96_24.flac
# O pause nao pode descartar nem duplicar bytes do ring buffer: o dump com uma pausa
# injetada no meio tem de sair identico ao dump sem pausa.
verify-pause: rust
	@test -n "$(FILE)" || { echo "uso: make verify-pause FILE=arquivo.flac"; exit 2; }
	@./$(RUST_BIN) --dump /tmp/hog_sem_pausa.raw "$(FILE)" >/dev/null
	@./$(RUST_BIN) --dump /tmp/hog_com_pausa.raw --pause-at 100000 "$(FILE)" >/dev/null
	@test -s /tmp/hog_sem_pausa.raw || { echo "FALHOU: dump vazio"; exit 1; }
	@test -s /tmp/hog_com_pausa.raw || { echo "FALHOU: dump vazio"; exit 1; }
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
	@test -s /tmp/hog_vol90.raw || { echo "FALHOU: dump vazio"; exit 1; }
	@cmp /tmp/hog_vol20.raw /tmp/hog_vol90.raw \
	  && echo "volume: amostras identicas a 20% e 90%"

# make info FILE=musicas/faixa.flac — mostra o que seria negociado, sem tocar no device.
info: cpp
	@test -n "$(FILE)" || { echo "uso: make info FILE=caminho/do/arquivo.flac"; exit 2; }
	@./$(CPP_BUILD)/hog-audio --info "$(FILE)"

# make play FILE=musicas/faixa.flac
play: cpp
	@test -n "$(FILE)" || { echo "uso: make play FILE=caminho/do/arquivo.flac"; exit 2; }
	@./$(CPP_BUILD)/hog-audio "$(FILE)"

# Monta o .app: sem Info.plist o macOS trata o binário como processo de segundo plano e
# ele nunca abre janela nem recebe foco de teclado.
app: bindings
	@cd apps/player && swift build -c release
	@mkdir -p $(APP_DIR)/Contents/MacOS
	@cp apps/player/Resources/Info.plist $(APP_DIR)/Contents/Info.plist
	@cp apps/player/.build/release/HogPlayer $(APP_DIR)/Contents/MacOS/HogAudio
	@echo "app montado em $(APP_DIR)"

# make run-app FILE=musicas/faixa.flac
run-app: app
	@test -n "$(FILE)" || { echo "uso: make run-app FILE=caminho/do/arquivo.flac"; exit 2; }
	@$(APP_DIR)/Contents/MacOS/HogAudio "$(FILE)"

clean:
	@rm -rf $(CPP_BUILD) cpp/build-asan rust/target
