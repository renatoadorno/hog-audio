# Documentação

Duas naturezas diferentes moram aqui, e confundi-las é caro: uma descreve o projeto **como ele
é**, e a outra registra **como ele chegou aqui**.

## `adr/` — decisões vivas

Descrevem o desenho vigente. São o que se lê para entender o projeto hoje, e o que se atualiza
quando a arquitetura muda. Um ADR que deixou de valer não é apagado: ganha `Superseded` no
Status, com link para quem o substituiu — o raciocínio descartado costuma valer tanto quanto o
adotado.

| ADR | Assunto | Status |
|---|---|---|
| [0001](adr/0001-arquitetura-cpp-rust-swift.md) | Arquitetura em hub: C++ no core, Rust nas integrações, Swift na interface | Superseded pelo 0002 |
| [0002](adr/0002-consolidar-o-core-em-rust.md) | Consolidar o core em Rust e aposentar o C++ | **Aceito** |

## `superpowers/` — registro dos trabalhos

Planos e specs de execução, um par por trabalho, com a data no nome. São **histórico**: valem
como registro do que foi decidido e medido na época, e envelhecem de propósito. Não os
atualize para refletir o presente — quando o desenho muda, o lugar de escrever é um ADR.

| Trabalho | Spec | Plano |
|---|---|---|
| Port para Rust (16/08) | [design](superpowers/specs/2026-08-16-port-rust-design.md) · [resultado](superpowers/specs/2026-08-16-port-rust-resultado.md) | [plano](superpowers/plans/2026-08-16-port-rust.md) |
| Mini player SwiftUI (18/08) | [design](superpowers/specs/2026-08-18-mini-player-design.md) · [resultado](superpowers/specs/2026-08-18-mini-player-resultado.md) | [plano](superpowers/plans/2026-08-18-mini-player.md) |
| Fila de faixas (19/08) | [design](superpowers/specs/2026-08-19-playlist-design.md) | — (não implementado) |

Ao ler qualquer coisa em `superpowers/`, confira antes se um ADR já mudou a conclusão. O
design do port, por exemplo, descreve as duas implementações convivendo em `cpp/` e `rust/`:
verdade em 16/08, superada pelo ADR 0002.
