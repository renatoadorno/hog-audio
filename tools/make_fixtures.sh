#!/usr/bin/env bash
# Gera os arquivos de áudio que os testes consomem.
#
# Áudio não entra no repo — nem as fixtures. O que entra é esta receita: ela é a fonte da
# verdade sobre o que cada arquivo contém, e um clone limpo a executa uma vez antes de
# `make test`. Sem isto, os testes que dependem de fixture falham dizendo o que rodar.
#
# Todo o conteúdo é sintetizado pelo ffmpeg: nada aqui depende da coleção de música de
# ninguém, e por isso a suíte é reprodutível em qualquer máquina.
#
# Os canais levam frequências diferentes de propósito (440 Hz à esquerda, 660 à direita):
# um defeito que troque, duplique ou some os canais produz um arquivo audivelmente distinto,
# e o `make verify` — que compara contra o PCM que o próprio ffmpeg extrai — o denuncia.
set -euo pipefail

DEST="${1:-testdata}"
FORCE="${FORCE:-0}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

command -v ffmpeg >/dev/null || { echo "falta ffmpeg (brew install ffmpeg)"; exit 1; }
mkdir -p "$DEST"

# $1 = arquivo, $2 = sample rate, $3 = bits, $4 = canais, $5 = duração em segundos
tom() {
  local nome=$1 rate=$2 bits=$3 canais=$4 dur=$5 destino="$DEST/$1"
  if [ -f "$destino" ] && [ "$FORCE" != "1" ]; then
    echo "  = $nome (já existe)"
    return
  fi
  local fmt="s16" ; [ "$bits" = "24" ] && fmt="s32"
  if [ "$canais" = "1" ]; then
    ffmpeg -v error -y -f lavfi -i "sine=frequency=440:duration=$dur:sample_rate=$rate" \
      -c:a flac -sample_fmt "$fmt" -bits_per_raw_sample "$bits" "$destino"
  else
    ffmpeg -v error -y \
      -f lavfi -i "sine=frequency=440:duration=$dur:sample_rate=$rate" \
      -f lavfi -i "sine=frequency=660:duration=$dur:sample_rate=$rate" \
      -filter_complex "[0:a][1:a]amerge=inputs=2[a]" -map "[a]" \
      -c:a flac -sample_fmt "$fmt" -bits_per_raw_sample "$bits" "$destino"
  fi
  echo "  + $nome ($rate Hz / $bits bits / $canais canais / ${dur}s)"
}

# O FLAC guarda a capa como `METADATA_BLOCK_PICTURE`: um Vorbis comment cujo valor é o bloco
# PICTURE do FLAC em base64. O `-disposition:v attached_pic` do ffmpeg escreve um caminho
# diferente (stream de vídeo anexado) que o AVFoundation não devolve na leitura de metadados —
# então a fixture não exercitaria o código que lê capa de FLAC de verdade. Daí montar o bloco
# à mão.
bloco_de_capa() {
  local jpeg=$1
  python3 - "$jpeg" <<'PYEOF'
import base64, struct, sys

data = open(sys.argv[1], "rb").read()
mime, desc = b"image/jpeg", b""
bloco = (
    struct.pack(">I", 3)                       # tipo 3 = capa frontal
    + struct.pack(">I", len(mime)) + mime
    + struct.pack(">I", len(desc)) + desc
    + struct.pack(">IIII", 100, 100, 24, 0)    # largura, altura, profundidade, cores
    + struct.pack(">I", len(data)) + data
)
sys.stdout.write(base64.b64encode(bloco).decode("ascii"))
PYEOF
}

# $1 = arquivo, $2 = codec, $3 = com capa (1) ou sem (0)
com_tags() {
  local nome=$1 codec=$2 capa=$3 destino="$DEST/$1"
  if [ -f "$destino" ] && [ "$FORCE" != "1" ]; then
    echo "  = $nome (já existe)"
    return
  fi
  local extra=()
  [ "$codec" = "libmp3lame" ] && extra=(-id3v2_version 3)
  if [ "$capa" = "1" ] && [ "$codec" = "flac" ]; then
    local jpeg="$TMP/capa.jpg"
    ffmpeg -v error -y -f lavfi -i "color=c=orange:s=100x100:d=1" -frames:v 1 "$jpeg"
    ffmpeg -v error -y \
      -f lavfi -i "sine=frequency=440:duration=2:sample_rate=44100" \
      -c:a flac \
      -metadata title="Titulo Teste" -metadata artist="Artista Teste" \
      -metadata album="Album Teste" \
      -metadata "METADATA_BLOCK_PICTURE=$(bloco_de_capa "$jpeg")" "$destino"
  elif [ "$capa" = "1" ]; then
    ffmpeg -v error -y \
      -f lavfi -i "sine=frequency=440:duration=2:sample_rate=44100" \
      -f lavfi -i "color=c=orange:s=300x300:d=1" \
      -map 0:a -map 1:v -frames:v 1 -c:a "$codec" -c:v mjpeg \
      -disposition:v attached_pic ${extra[@]+"${extra[@]}"} \
      -metadata title="Titulo Teste" -metadata artist="Artista Teste" \
      -metadata album="Album Teste" "$destino"
  else
    ffmpeg -v error -y \
      -f lavfi -i "sine=frequency=440:duration=2:sample_rate=44100" \
      -c:a "$codec" ${extra[@]+"${extra[@]}"} \
      -metadata title="Titulo Teste" -metadata artist="Artista Teste" \
      -metadata album="Album Teste" "$destino"
  fi
  echo "  + $nome (tags$([ "$capa" = 1 ] && echo " + capa"))"
}

echo "fixtures de formato (negociação, ring buffer, bit-perfect):"
tom t44_16.flac       44100  16 2  6
tom t44_longo.flac    44100  16 2 25
tom t48_16_mono.flac  48000  16 1  6
tom t96_24.flac       96000  24 2  6
tom t96_longo.flac    96000  24 2 20
tom t192_24.flac     192000  24 2  6

echo "fixtures de metadados (leitura de tags e capa pelo AVFoundation):"
com_tags meta_fixture.flac          flac       1
com_tags meta_fixture_sem_capa.flac flac       0
com_tags meta_fixture.m4a           alac       1
com_tags meta_fixture.mp3           libmp3lame 1

echo
echo "pronto. FORCE=1 $0 regera o que já existe."
