# Orquestra as duas implementações e a comparação entre elas.
CPP_BUILD := cpp/build
RUST_BIN  := rust/target/release/hog-audio

.PHONY: all cpp cpp-test cpp-asan rust rust-test test verify info play clean

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

test: cpp-test rust-test

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

# make info FILE=musicas/faixa.flac — mostra o que seria negociado, sem tocar no device.
info: cpp
	@test -n "$(FILE)" || { echo "uso: make info FILE=caminho/do/arquivo.flac"; exit 2; }
	@./$(CPP_BUILD)/hog-audio --info "$(FILE)"

# make play FILE=musicas/faixa.flac
play: cpp
	@test -n "$(FILE)" || { echo "uso: make play FILE=caminho/do/arquivo.flac"; exit 2; }
	@./$(CPP_BUILD)/hog-audio "$(FILE)"

clean:
	@rm -rf $(CPP_BUILD) cpp/build-asan rust/target
