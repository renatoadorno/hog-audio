# Domain docs

## Antes de explorar o código

Leia, quando existirem:

- `CONTEXT-MAP.md` na raiz e o `CONTEXT.md` de cada contexto pertinente;
- ADRs de sistema em `docs/adr/`;
- ADRs específicas do contexto, em `<contexto>/docs/adr/`.

Se não existirem, prossiga silenciosamente. Não sugira criá-los antecipadamente; o skill de modelagem de domínio os cria quando conceitos ou decisões forem efetivamente definidos.

## Layout

Este é um repositório multi-context:

```
/
├── CONTEXT-MAP.md
├── docs/adr/                         ← decisões de sistema
├── rust/
│   ├── CONTEXT.md
│   ├── src/
│   └── docs/adr/                    ← decisões do core
└── apps/player/
    ├── CONTEXT.md
    └── docs/adr/                    ← decisões do player/UI
```

## Vocabulário e decisões

Use os termos definidos no `CONTEXT.md` do contexto adequado ao nomear conceitos de domínio. Se uma mudança contradisser uma ADR, deixe o conflito explícito em vez de substituí-la silenciosamente.
