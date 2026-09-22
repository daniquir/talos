#!/usr/bin/env bash
# Build a store-ready zip of talos-extension (Chrome Web Store / AMO upload).
set -euo pipefail

EXT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="$(cd "$EXT/.." && pwd)"
OUT_DIR="$REPO/dist"
VERSION="$(node -pe "JSON.parse(require('fs').readFileSync('$EXT/manifest.json','utf8')).version")"
ZIP="$OUT_DIR/talos-extension-v${VERSION}.zip"

mkdir -p "$OUT_DIR"
rm -f "$ZIP"

(
  cd "$EXT"
  zip -r "$ZIP" . \
    -x "*.DS_Store" \
    -x "**/.git/**" \
    -x "**/node_modules/**" \
    -x "**/*.map" \
    -x "scripts/*" \
    -x "README.md"
)

echo "Packed $ZIP"
ls -lh "$ZIP"
