# Atalhos para o build de verdade, que é o CMake.

BUILD      := build
BUILD_ASAN := build-asan

.PHONY: all build test test-asan asan info clean re

all: build

build:
	@cmake -B $(BUILD) -DCMAKE_BUILD_TYPE=Release >/dev/null
	@MAKEFLAGS= cmake --build $(BUILD)

# Testes do núcleo puro (negociação de formato e ring buffer).
test: build
	@ctest --test-dir $(BUILD) --output-on-failure

# Os mesmos testes sob AddressSanitizer e UBSan: o ring buffer roda em thread de tempo
# real, e um transbordo ali corrompe memória sem que os asserts percebam.
asan:
	@cmake -B $(BUILD_ASAN) -DCMAKE_BUILD_TYPE=Debug -DHOG_SANITIZE=ON >/dev/null
	@MAKEFLAGS= cmake --build $(BUILD_ASAN)

test-asan: asan
	@ctest --test-dir $(BUILD_ASAN) --output-on-failure

# make info FILE=musicas/faixa.flac — mostra o que seria negociado, sem tocar no device.
info: build
	@test -n "$(FILE)" || { echo "uso: make info FILE=caminho/do/arquivo.flac"; exit 2; }
	@./$(BUILD)/hog-audio --info "$(FILE)"

# make play FILE=musicas/faixa.flac
play: build
	@test -n "$(FILE)" || { echo "uso: make play FILE=caminho/do/arquivo.flac"; exit 2; }
	@./$(BUILD)/hog-audio "$(FILE)"

clean:
	@rm -rf $(BUILD) $(BUILD_ASAN)

re: clean all
