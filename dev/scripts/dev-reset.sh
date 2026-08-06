#!/usr/bin/env bash
# Wipe DEV volumes and rebootstrap with fresh sample data.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

echo "==> Stopping stack"
"$ROOT/dev/scripts/dev-down.sh" -v || true

echo "==> Wiping data-dev (keeps SSH placeholder if present)"
# Preserve SSH key file path for compose mounts; wipe everything else.
SSH_BACKUP=""
if [[ -f data-dev/ssh/id_rsa_talos ]]; then
  SSH_BACKUP="$(mktemp)"
  cp -a data-dev/ssh/id_rsa_talos "$SSH_BACKUP"
  cp -a data-dev/ssh/id_rsa_talos.pub "${SSH_BACKUP}.pub" 2>/dev/null || true
fi

# Prefer docker for root-owned files from previous runs
docker run --rm -v "$ROOT/data-dev:/data" alpine sh -c 'rm -rf /data/*' 2>/dev/null \
  || rm -rf data-dev/*

mkdir -p data-dev/{web,password-store,ssh,bunker-gnupg}
if [[ -n "$SSH_BACKUP" ]]; then
  mv "$SSH_BACKUP" data-dev/ssh/id_rsa_talos
  [[ -f "${SSH_BACKUP}.pub" ]] && mv "${SSH_BACKUP}.pub" data-dev/ssh/id_rsa_talos.pub
  chmod 600 data-dev/ssh/id_rsa_talos
fi

echo "==> Rebuilding from scratch"
"$ROOT/dev/scripts/dev-up.sh"
