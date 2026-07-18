#!/bin/bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SVG="$REPO_ROOT/data/icons/hicolor/scalable/apps/io.missioncenter.MissionCenter.svg"
ICONSET="$REPO_ROOT/build-macos/MissionCenter.iconset"
ICNS="$REPO_ROOT/build-macos/MissionCenter.icns"

mkdir -p "$ICONSET"

for size in 16 32 64 128 256 512; do
  rsvg-convert -w "$size" -h "$size" "$SVG" -o "$ICONSET/icon_${size}x${size}.png"
  rsvg-convert -w $((size * 2)) -h $((size * 2)) "$SVG" -o "$ICONSET/icon_${size}x${size}@2x.png"
done

iconutil -c icns -o "$ICNS" "$ICONSET"
rm -rf "$ICONSET"

echo "Generated: $ICNS"
