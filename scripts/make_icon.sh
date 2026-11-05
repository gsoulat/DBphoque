#!/usr/bin/env bash
# Génère les icônes de DBphoque à partir de assets/icon.svg (macOS uniquement :
# l'icns passe par iconutil) ; demande rsvg-convert et ImageMagick :
#   brew install librsvg imagemagick
#   assets/icon.png      1024 px, icône du Dock
#   assets/icon-256.png  256 px, icône de fenêtre (incluse dans le binaire)
#   assets/icon.icns     jeu complet pour le bundle macOS
#   assets/icon.ico      installeur et raccourcis Windows (16 à 256 px)
set -euo pipefail
cd "$(dirname "$0")/.."
svg=assets/icon.svg
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

render() { # render <taille> <sortie>
  # rsvg-convert garde la transparence (Quick Look remplissait les coins en blanc).
  rsvg-convert -w "$1" -h "$1" "$svg" -o "$2"
}

iconset="$tmp/icon.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  render "$size" "$iconset/icon_${size}x${size}.png"
  render "$((size * 2))" "$iconset/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$iconset" -o assets/icon.icns
cp "$iconset/icon_512x512@2x.png" assets/icon.png
cp "$iconset/icon_256x256.png" assets/icon-256.png
magick "$iconset/icon_16x16.png" "$iconset/icon_32x32.png" \
  "$iconset/icon_32x32@2x.png" "$iconset/icon_128x128.png" "$iconset/icon_256x256.png" assets/icon.ico
ls -la assets/icon.png assets/icon-256.png assets/icon.icns assets/icon.ico
