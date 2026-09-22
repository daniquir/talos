#!/usr/bin/env bash
# Serve the fake login page for extension autofill tests (dev only).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${PORT:-8765}"
DIR="$ROOT/fixtures/fake-login"

if [[ ! -f "$DIR/index.html" ]]; then
  echo "Missing $DIR/index.html" >&2
  exit 1
fi

echo "Fake login (dev): http://127.0.0.1:${PORT}/"
echo "Seed secret URL should be: http://127.0.0.1:${PORT}"
echo "Ctrl+C to stop."
echo

cd "$DIR"
exec python3 -m http.server "$PORT" --bind 127.0.0.1
