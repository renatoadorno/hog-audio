# Issue tracker: GitHub

Issues e especificações deste repositório vivem no GitHub Issues. Use a CLI `gh` para todas as operações.

## Convenções

- **Criar issue:** `gh issue create --title "..." --body "..."`.
- **Ler issue:** `gh issue view <número> --comments`, incluindo labels.
- **Listar issues:** `gh issue list --state open --json number,title,body,labels,comments`, aplicando filtros de estado e label quando necessário.
- **Comentar:** `gh issue comment <número> --body "..."`.
- **Aplicar/remover labels:** `gh issue edit <número> --add-label "..."` / `--remove-label "..."`.
- **Fechar:** `gh issue close <número> --comment "..."`.

A CLI `gh` infere o repositório pelo remoto GitHub quando executada dentro do clone. Este repositório ainda não possui remoto configurado; configure-o antes de usar operações remotas.

## Pull requests como superfície de triagem

**PRs como superfície de solicitações: não.**

Quando um skill disser “publicar no rastreador de issues”, crie uma GitHub Issue. Quando disser “buscar o ticket relevante”, execute `gh issue view <número> --comments`.
