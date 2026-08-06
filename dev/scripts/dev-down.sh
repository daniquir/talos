#!/usr/bin/env bash
# Stop the Talos development stack.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

docker compose -f docker-compose.yaml -f docker-compose.dev.yaml --env-file .env.dev down "$@"
echo "Talos DEV stopped."
