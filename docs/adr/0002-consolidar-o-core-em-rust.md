# 2. Consolidar o core em Rust e aposentar o C++

Data: 2026-08-19

## Status

Aceito. **Supersede o [ADR 0001](0001-arquitetura-cpp-rust-swift.md).**

## Contexto

O ADR 0001 desenhou uma topologia em hub com o C++ no core, o Rust nas integrações de rede e
o Swift orquestrando. Ele registrou explicitamente que "reescrever tudo em Rust" continuava
aberta como alternativa, e o motivo de não a ter escolhido na hora: preservação de risco. O
core C++ já tinha resolvido armadilhas do Core Audio que ninguém acerta de primeira, e essa
experiência valia mais que o código.

Três coisas aconteceram desde então, e todas são fato verificado, não previsão:

1. **O port para Rust foi feito e provado.** O `--dump` grava os bytes que iriam ao IOProc, e
   o `ffmpeg` extrai o PCM de referência do mesmo arquivo. As três comparações — C++ contra o
   oráculo, Rust contra o oráculo, e um contra o outro — bateram amostra a amostra em
   1.152.000 amostras. Registrado em
   [`port-rust-resultado.md`](../superpowers/specs/2026-08-16-port-rust-resultado.md).
2. **O Rust pegou um defeito que o C++ escondia.** O mesmo bug de wrap-around no ring buffer
   era `heap-buffer-overflow` silencioso em C++ — passava no teste ingênuo e só aparecia sob
   AddressSanitizer — e virou panic imediato em Rust. A classe inteira de corrupção silenciosa
   mudou de categoria.
3. **A interface gráfica foi construída sobre o Rust, não sobre o C++.** O `apps/player` fala
   com a lib Rust por `uniffi`. A ordem de implementação do ADR 0001 previa "Swift ↔ C++
   direto" como primeiro passo; o que se construiu foi Swift ↔ Rust, e funcionou.

A partir daí o C++ passou a ser uma segunda implementação completa que nada consome: a CLI
Rust tem paridade de flags (`--info`, `--volume`, `--max-volume`, `--dump`, `--pause-at`), o
app não o referencia, e o último commit que o tocou é de 16/08.

## Decisão

O **Rust é o core**: HAL, hog mode, lock de formato, IOProc, ring buffer, negociação,
decodificação e a CLI. O **Swift é a interface**, e fala com o core por `uniffi`. Não há
terceira linguagem.

```
Swift (UI/UX)
  └── Rust core ──> DAC        [uniffi: controle e leitura de estado, nunca áudio]
```

As 2.163 linhas de C++ foram removidas. O histórico continua no git: `git show 1850a56:cpp/`
lista a última árvore viva, e `git checkout 1850a56 -- cpp/` a traz de volta inteira se algum
dia for preciso.

### O que continua valendo do ADR 0001

A **regra de tempo real** sobrevive intacta e é a parte que mais importa: o IOProc nunca
atravessa fronteira de linguagem, nunca aloca, nunca trava um mutex. Ele lê do ring buffer e
escreve no buffer do device — só isso. O ADR 0001 já alertava que "em Rust é igualmente fácil
alocar sem perceber dentro de um callback", e continua verdade.

A **fronteira Swift↔Rust por `uniffi`** também continua como estava: controle e leitura de
estado, nunca dados de áudio.

O que cai é a camada C++ e a fronteira Rust↔C++ que o ADR 0001 desenhava para o áudio do
librespot. Quando a integração com Spotify entrar, o librespot vira mais um produtor
escrevendo no mesmo ring buffer — dentro do mesmo processo e da mesma linguagem, sem ponte.

## Consequências

### Positivas

- **Duas toolchains em vez de três** (Cargo e SPM; o CMake sai). O ADR 0001 registrava as três
  como o custo real do desenho.
- **Uma implementação, não duas.** Toda correção passa a valer uma vez. Enquanto as duas
  coexistiram, cada mudança no caminho do áudio tinha de ser feita em dois lugares ou aceitar
  divergência.
- **Depuração numa linguagem só.** O ADR 0001 aceitava conscientemente que "um crash no IOProc
  alimentado por Rust e disparado pelo Swift é consideravelmente mais caro de investigar".
  Esse custo desaparece.
- **A porta para o librespot fica mais simples**, não mais difícil: ele é Rust, e agora o core
  também.

### Negativas, aceitas conscientemente

- **A comparação cruzada C++ ↔ Rust do `make verify` acaba.** Sobra a comparação que de fato
  prova bit-perfect: Rust contra o PCM que o `ffmpeg` extrai. O oráculo sempre foi o ffmpeg —
  a coluna C++ era instrumento do port, e o port terminou.
- **O `make cpp-asan` sai, e com ele o único sanitizer do projeto.** A perda é menor do que
  parece: ele rodava os testes *do C++*, e nunca cobriu uma linha do que executa hoje. O que
  cobre o `unsafe` do Rust agora é o `cargo clippy -D warnings` no `make test` — incluindo
  `not_unsafe_ptr_arg_deref`, que existe exatamente para esta classe de código.
- **Se algum dia aparecer uma dependência que só exista em C++**, a economia desaparece e o
  desenho de três camadas volta a ser o certo. Nenhuma está prevista.

## Alternativas consideradas

**Manter o C++ congelado onde está.** Custo zero de execução, custo alto de leitura: o
Makefile e o README ficam bifurcados, e todo leitor — humano ou agente — trata 2.163 linhas
mortas como contexto vivo. Foi o estado entre 16/08 e 19/08, e é o que este ADR encerra.

**Manter só o `make verify` cruzado, apagando o resto.** Não se sustenta: para o binário C++
existir é preciso manter todo o `cpp/` compilando, ou seja, o custo inteiro em troca de uma
comparação cujo valor probatório já vem do ffmpeg.

## Referências

- [ADR 0001 — Arquitetura em hub](0001-arquitetura-cpp-rust-swift.md) (superseded)
- [Resultado do port para Rust](../superpowers/specs/2026-08-16-port-rust-resultado.md)
- [Design do mini player](../superpowers/specs/2026-08-18-mini-player-design.md)
