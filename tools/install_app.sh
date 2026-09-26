#!/usr/bin/env bash
# Builda o app, encerra uma instalação antiga com segurança e instala/atualiza o bundle.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="$ROOT/apps/player/HogAudio.app"
INSTALL_DIR="${HOG_AUDIO_INSTALL_DIR:-/Applications}"
DEST="$INSTALL_DIR/HogAudio.app"
OPEN_APP="${OPEN_APP:-1}"

case "$DEST" in
  /Applications/HogAudio.app|"$HOME"/Applications/HogAudio.app) ;;
  *)
    echo "destino recusado por segurança: $DEST" >&2
    echo "use /Applications ou $HOME/Applications" >&2
    exit 2
    ;;
esac

command -v ditto >/dev/null || { echo "falta ditto (incluído no macOS)" >&2; exit 1; }
command -v open >/dev/null || { echo "falta open (incluído no macOS)" >&2; exit 1; }

printf '==> Buildando HogAudio\n'
make -C "$ROOT" app

tmp="$(mktemp -d "${TMPDIR:-/tmp}/hog-audio-install.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

ditto "$SOURCE" "$tmp/HogAudio.app"

# O app trata SIGTERM e restaura sample rate, formato, volume e hog mode antes de sair.
pids="$(pgrep -f '/HogAudio\.app/Contents/MacOS/HogAudio$' || true)"
if [[ -n "$pids" ]]; then
  printf '==> Encerrando a versão em execução\n'
  while IFS= read -r pid; do
    [[ -n "$pid" ]] && kill -TERM "$pid"
  done <<< "$pids"

  for _ in {1..100}; do
    pgrep -f '/HogAudio\.app/Contents/MacOS/HogAudio$' >/dev/null || break
    sleep 0.05
  done
  if pgrep -f '/HogAudio\.app/Contents/MacOS/HogAudio$' >/dev/null; then
    echo "o app não encerrou; instalação cancelada para não substituir um bundle em uso" >&2
    exit 1
  fi
fi

printf '==> Instalando em %s\n' "$DEST"
mkdir -p "$INSTALL_DIR"
rm -rf "$DEST"
ditto "$tmp/HogAudio.app" "$DEST"

if [[ "$OPEN_APP" == "1" ]]; then
  printf '==> Abrindo HogAudio\n'
  open "$DEST"
fi

printf 'HogAudio instalado/atualizado em %s\n' "$DEST"
