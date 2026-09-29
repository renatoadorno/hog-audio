#!/usr/bin/env bash
# Gate do protótipo: o core não muda para servir a UI, e o core compilado dentro do protótipo
# usa as mesmas dependências (nome, versão e features) que o `make verify*` provou, salvo as
# exceções justificadas em core_version_exceptions.txt.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"

if ! git -C "$root" diff --quiet main -- rust/; then
  echo "FALHOU: o core mudou em relação à main (git diff main -- rust/)"
  exit 1
fi
if [ -n "$(git -C "$root" status --porcelain -- rust/)" ]; then
  echo "FALHOU: há arquivos novos ou alterados em rust/"
  exit 1
fi

# Árvore do core no alvo do app, só com o que entra no lib (dependências normais e de build), uma
# linha "pacote|valor" por pacote. O valor junta cada versão com as features ativadas, porque o
# mesmo pacote pode aparecer uma vez para o alvo e outra para o host. No workspace do protótipo,
# `-p hog-audio` resolve as features com todos os membros juntos, como o `--workspace` do
# `gpui-test` compila. O `--locked` impede o cargo de reescrever qualquer um dos dois locks.
# Só o caminho do próprio core vira "(core)"; qualquer outro source (path, git) fica visível,
# para um `[patch]` por path num pacote do core aparecer como diferença.
core_tree() {
  cargo tree --manifest-path "$1" -p hog-audio --target aarch64-apple-darwin -e normal,build \
    --locked --prefix none --format '{p}|{f}' |
    sed -E -e 's/ \(\*\)$//' -e 's/ \(proc-macro\)//' -e 's/^([^ ]+) v/\1 /' |
    awk -v core_source=" ($root/rust)" '{
      i = index($0, core_source)
      if (i) $0 = substr($0, 1, i - 1) " (core)" substr($0, i + length(core_source))
      print
    }' |
    LC_ALL=C sort -u |
    awk -F'|' '{
      name = $1
      sub(/ .*/, "", name)
      entry = substr($1, length(name) + 2) " [" $2 "]"
      value[name] = (name in value) ? value[name] " ; " entry : entry
    } END { for (name in value) print name "|" value[name] }'
}

if ! core="$(core_tree "$root/rust/Cargo.toml")"; then
  echo "FALHOU: o cargo tree --locked não resolveu a árvore do core em rust/"
  exit 1
fi
if ! proto="$(core_tree "$root/apps/gpui/Cargo.toml")"; then
  echo "FALHOU: o cargo tree --locked não resolveu a árvore do core em apps/gpui/"
  exit 1
fi

# Exceções: "pacote | valor no core | valor no protótipo | motivo", com os valores como o gate os
# imprime. O formato está documentado no topo do arquivo. Sem o arquivo, a lista é vazia.
exceptions="$root/apps/gpui/tools/core_version_exceptions.txt"
[ -f "$exceptions" ] || exceptions=/dev/null

# Se o awk falhar (arquivo de exceções ilegível, por exemplo), o `set -e` sairia sem mensagem.
if ! failures="$(
  awk -F'|' '
    function trim(s) { gsub(/^[ \t]+|[ \t]+$/, "", s); return s }
    function show(v) { return v == "" ? "ausente" : v }
    BEGIN { q = "\047"; list = "core_version_exceptions.txt" }
    FILENAME == ARGV[1] { core[$1] = $2; names[$1] = 1; next }
    FILENAME == ARGV[2] { proto[$1] = $2; names[$1] = 1; next }
    /^[ \t]*(#|$)/ { next }
    {
      name = trim($1)
      if (NF != 4 || name == "" || trim($4) == "") {
        print "FALHOU: a linha " FNR " de " list " não segue o formato"
      } else if (name == "coreaudio-sys" || name ~ /^uniffi/) {
        print "FALHOU: " name " nunca pode ser exceção (" list ")"
      } else if (name in expected_core) {
        print "FALHOU: " name " aparece duas vezes em " list
      } else {
        expected_core[name] = trim($2)
        expected_proto[name] = trim($3)
      }
    }
    END {
      for (name in names) {
        c = show(core[name])
        p = show(proto[name])
        if (c == p) continue
        if (!(name in expected_core)) {
          print "FALHOU: " name " é " q c q " no core e " q p q " no protótipo"
        } else if (expected_core[name] != c || expected_proto[name] != p) {
          print "FALHOU: " name " é " q c q " no core e " q p q " no protótipo, mas " list \
            " espera " q expected_core[name] q " e " q expected_proto[name] q
        }
      }
      for (name in expected_core) {
        if (show(core[name]) == show(proto[name])) {
          print "FALHOU: a exceção de " name " em " list " já não difere; tire-a da lista"
        }
      }
    }
  ' <(printf '%s\n' "$core") <(printf '%s\n' "$proto") "$exceptions" | LC_ALL=C sort
)"; then
  echo "FALHOU: não consegui ler core_version_exceptions.txt ou comparar as árvores"
  exit 1
fi

if [ -n "$failures" ]; then
  printf '%s\n' "$failures"
  exit 1
fi

echo "core intocado; árvore do core (nome, versão e features) igual à do rust/Cargo.lock, salvo as exceções listadas"
