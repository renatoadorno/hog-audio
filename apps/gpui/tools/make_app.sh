#!/usr/bin/env bash
# Monta o .app do protótipo em apps/gpui/build/. Não instala: o /Applications é do app SwiftUI,
# e sem Info.plist o macOS trata o binário como processo de segundo plano.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
app="$root/apps/gpui/build/HogAudioGPUI.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/apps/gpui/app/Resources/Info.plist" "$app/Contents/Info.plist"
cp "$root/apps/player/Resources/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
cp "$root/apps/gpui/target/release/hog-player-gpui" "$app/Contents/MacOS/HogAudioGPUI"
echo "app montado em $app"
