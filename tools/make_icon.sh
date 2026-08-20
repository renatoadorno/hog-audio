#!/usr/bin/env bash
# Gera o .icns do app a partir do SVG da logo.
#
# O conteúdo é renderizado a 824 de 1024 e centralizado: é a proporção que a Apple usa nos
# ícones do sistema, e sem ela o ícone fica visivelmente maior que os vizinhos no Dock. O SVG
# já traz os cantos arredondados na proporção certa (raio 229/1024 = 22,4%, contra os 22,5%
# do padrão), então só faltava a margem.
#
# Cada tamanho é renderizado direto do vetor em vez de reduzido do maior: nos 16 e 32 px o
# traçado do desenho é fino demais para sobreviver a um downsample.
#
# Precisa de rsvg-convert (brew install librsvg). O .icns fica versionado, então montar o app
# não depende desta ferramenta — só regenerar depois de mexer no SVG.
set -euo pipefail

SVG="${1:-logos/hog-icon-dark.svg}"
DEST="${2:-apps/player/Resources/AppIcon.icns}"

command -v rsvg-convert >/dev/null || { echo "falta rsvg-convert (brew install librsvg)"; exit 1; }
test -f "$SVG" || { echo "svg não encontrado: $SVG"; exit 1; }

ICONSET="$(mktemp -d)/HogAudio.iconset"
mkdir -p "$ICONSET"

render() { # $1 = lado do canvas em px, $2 = arquivo destino
  local canvas=$1 dest=$2 content offset
  content=$(( canvas * 824 / 1024 ))
  offset=$(( (canvas - content) / 2 ))
  rsvg-convert -w "$content" -h "$content" \
    --page-width "$canvas" --page-height "$canvas" \
    --top "$offset" --left "$offset" \
    "$SVG" -o "$dest"
}

render 16   "$ICONSET/icon_16x16.png"
render 32   "$ICONSET/icon_16x16@2x.png"
render 32   "$ICONSET/icon_32x32.png"
render 64   "$ICONSET/icon_32x32@2x.png"
render 128  "$ICONSET/icon_128x128.png"
render 256  "$ICONSET/icon_128x128@2x.png"
render 256  "$ICONSET/icon_256x256.png"
render 512  "$ICONSET/icon_256x256@2x.png"
render 512  "$ICONSET/icon_512x512.png"
render 1024 "$ICONSET/icon_512x512@2x.png"

mkdir -p "$(dirname "$DEST")"
iconutil -c icns "$ICONSET" -o "$DEST"
echo "ícone gerado em $DEST"
