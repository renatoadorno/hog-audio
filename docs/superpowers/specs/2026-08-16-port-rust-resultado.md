# Resultado do port para Rust

Data: 2026-08-16
Spec: [2026-08-16-port-rust-design.md](2026-08-16-port-rust-design.md)

## Resposta curta

**Rust cumpre bit-perfect, e o HAL não foi obstáculo.** As três comparações previstas no
critério de sucesso bateram, em três arquivos diferentes, incluindo 40 milhões de amostras de
uma faixa real.

## A prova

O ffmpeg extrai o PCM inteiro do FLAC; cada player grava, com `--dump`, os bytes que
entregaria ao IOProc — percorrendo o mesmo pipeline da reprodução (decodificador → ring buffer
→ consumidor com alinhamento de frame). O verificador converte o float32 de volta para inteiro
e compara amostra por amostra.

| Arquivo | Amostras | C++ vs ffmpeg | Rust vs ffmpeg | C++ vs Rust |
|---|---|---|---|---|
| `t96_24.flac` (96 kHz / 24 bits) | 1.152.000 | bit-perfect | bit-perfect | idênticos |
| `t44_16.flac` (44,1 kHz / 16 bits) | 528.984 | bit-perfect | bit-perfect | idênticos |
| `10-House-of-Memories.flac` (96/24) | 40.071.114 | bit-perfect | bit-perfect | idênticos |

Isso prova três coisas de uma vez: o C++ é bit-perfect contra referência externa, o Rust
também é, e a afirmação central do projeto — `int24 → float32 → int24` é exato — deixou de ser
documentação para virar fato medido. Antes deste experimento, essa premissa nunca havia sido
testada em nenhuma das implementações.

## Paridade funcional

- Saída de `--info` byte a byte idêntica entre os dois binários.
- Mensagens de erro idênticas em todos os casos de borda testados: recusa de 192 kHz,
  porcentagem fora da faixa, decibéis positivos, texto inválido.
- Reprodução real verificada no fone: hog obtido, rate travado, volume aplicado e device
  restaurado.
- `SIGINT` devolve exit 130, restaura rate, formato e volume.

## Esforço

| | C++ | Rust |
|---|---|---|
| Implementação | 1.532 linhas | 1.995 linhas |
| Testes | 631 linhas | 739 linhas |
| Dependências externas | nenhuma | `coreaudio-sys` |
| Testes automatizados | 3 binários / 127 checks | 50 testes + 2 de hardware |

O Rust ficou ~30% maior na implementação. As causas identificadas: tratamento explícito de
erro em cada chamada de FFI (onde o C++ encadeia condições), declarações de `unsafe impl Send`
/ `Sync` com justificativa, e a ausência de sobrecarga de operadores para os helpers de
propriedade.

### Onde o `unsafe` se concentra

| Módulo | Blocos `unsafe` | Natureza |
|---|---|---|
| `source.rs` | 12 | chamadas ao ExtendedAudioFile |
| `ffi.rs` | 9 | helpers de propriedade do HAL |
| `device.rs` | 5 | hog, formatos, IOProc |
| `ring.rs` | 4 | acesso ao buffer sob contrato SPSC |
| `main.rs` | 3 | IOProc, `signal`, ponteiro de contexto |
| `format.rs`, `volume.rs` | 0 | núcleo puro |

A previsão do spec se confirmou: a camada HAL é quase toda `unsafe`, e ali o Rust oferece
pouca proteção adicional. **Mas ela é 30% do código.** Os outros 70% — decisão de formato,
ring buffer, volume, orquestração — são seguros por construção, e é onde moraram os bugs reais
deste projeto.

## O achado mais relevante

O bug de wrap-around do ring buffer se comporta de forma **radicalmente diferente** nas duas
linguagens. Quebrando deliberadamente o cálculo da borda:

- **C++**: `heap-buffer-overflow` silencioso. O teste ingênuo de wrap-around **passava** —
  escrita e leitura cometiam o mesmo erro simetricamente e o dado "batia". Só foi exposto por
  AddressSanitizer somado a um teste especialmente desenhado (leitura fragmentada).
- **Rust**: panic imediato com bounds check, e **três** testes falham — incluindo o ingênuo,
  que no C++ não pegava nada. Sem sanitizer, sem teste especial.

Esse foi exatamente o bug mais caro do projeto, encontrado por revisão adversarial e invisível
com float32 (só apareceria num DAC com int24 packed). No Rust ele não teria sobrevivido ao
primeiro teste.

## Atritos encontrados no port

Nenhum bloqueante. Em ordem de tempo gasto:

1. **Linkagem do CoreFoundation.** `coreaudio-sys` linka CoreAudio e AudioToolbox, mas não
   CoreFoundation — necessário para `CFURL` e nomes de device. Resolvido com quatro linhas de
   `build.rs`.
2. **`AudioSource` não é `Send` por padrão.** O ponteiro opaco do ExtendedAudioFile impede
   mover o decodificador para a thread produtora sem uma declaração explícita. No C++ isso
   passou sem uma linha sequer; em Rust exigiu justificar por escrito por que é seguro.
3. **`extern "C"` agora exige `unsafe`** na edição atual do Rust.
4. **`coreaudio-rs` (alto nível) não serve.** Cobre AudioUnit, não o HAL: quem resolve é o
   `coreaudio-sys`. Avaliar apenas o crate de alto nível levaria à conclusão errada de que
   Rust não faz hog mode.

## Recomendação

A dúvida que motivou o experimento — "o HAL seria o problema de usar só Rust" — **não se
confirmou**. O Rust faz tudo que o C++ faz, com o mesmo resultado medido, e converte a classe
de bug mais perigosa deste projeto (corrupção silenciosa de memória em thread de tempo real)
em falha imediata e barulhenta.

Isso reabre a alternativa que o [ADR 0001](../../adr/0001-arquitetura-cpp-rust-swift.md)
registrou como descartada por preservação de risco: **duas linguagens (Rust + Swift) em vez de
três**. O argumento de "preservar código testado" perdeu força, porque o código agora existe
testado nas duas linguagens, com paridade comprovada byte a byte.

O que pesa de cada lado:

- **A favor de consolidar em Rust:** uma toolchain a menos, uma fronteira a menos, e o
  librespot passa a conviver no mesmo crate. A segurança de memória atua em 70% do código.
- **A favor de manter C++ no core:** ele funciona, está verificado, e o `unsafe` do HAL
  significa que a maior parte do risco remanescente não é mitigada pelo Rust de qualquer jeito.

Minha recomendação é **consolidar em Rust e aposentar o C++**, com uma ressalva: essa decisão
só se paga se o Swift falar direto com o Rust via uniffi. Se por algum motivo a UI acabar
precisando do C++, a economia desaparece e o desenho de três camadas do ADR 0001 volta a
fazer sentido. Vale decidir isso **antes** de investir na UI, não depois.

Nada foi removido: as duas implementações convivem em `cpp/` e `rust/`, e o `make verify`
compara as duas a qualquer momento.
