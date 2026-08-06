#!/usr/bin/env bash
# Bootstrap Talos local development stack (compose.dev + seed data).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

if [[ ! -f .env.dev ]]; then
  if [[ -f .env.dev.example ]]; then
    echo "==> Creating .env.dev from .env.dev.example"
    cp .env.dev.example .env.dev
  else
    echo "Missing .env.dev (and no .env.dev.example). Aborting." >&2
    exit 1
  fi
fi

COMPOSE=(docker compose -f docker-compose.yaml -f docker-compose.dev.yaml --env-file .env.dev)

echo "==> Preparing data-dev directories"
mkdir -p data-dev/{web,password-store,ssh,bunker-gnupg}

if [[ ! -f data-dev/ssh/id_rsa_talos ]]; then
  echo "==> Generating SSH placeholder key for storage mount"
  docker run --rm -v "$ROOT/data-dev/ssh:/ssh" alpine sh -c \
    'apk add --no-cache openssh-keygen >/dev/null && \
     ssh-keygen -t rsa -b 2048 -f /ssh/id_rsa_talos -N "" -C talos-dev && \
     chmod 600 /ssh/id_rsa_talos && chown 1000:1000 /ssh/id_rsa_talos'
fi

# SELinux: allow container read/write on bind mounts (Fedora/RHEL)
if command -v getenforce >/dev/null 2>&1 && [[ "$(getenforce 2>/dev/null)" == "Enforcing" ]]; then
  echo "==> Relabeling data-dev for containers (SELinux)"
  chcon -Rt container_file_t data-dev 2>/dev/null || true
fi

echo "==> Building and starting Talos (dev)"
"${COMPOSE[@]}" up --build -d

echo "==> Waiting for health"
for i in $(seq 1 90); do
  health="$(curl -sf http://localhost:3000/api/health 2>/dev/null || true)"
  if echo "$health" | grep -q '"storage":true' && ! echo "$health" | grep -q '"bunker":false'; then
    echo "    healthy: $health"
    break
  fi
  if [[ "$i" -eq 90 ]]; then
    echo "ERROR: services did not become healthy in time"
    "${COMPOSE[@]}" logs --tail=40
    exit 1
  fi
  sleep 2
done

echo "==> Seeding sample data"
"$ROOT/dev/scripts/dev-seed.sh"

echo ""
echo "Talos DEV is ready."
echo "  UI:         http://localhost:3000"
echo "  Master key: DevMasterKey-ChangeMe!"
echo "  GPG ID:     dev@talos.local"
echo ""
echo "Useful commands:"
echo "  ./dev/scripts/dev-seed.sh     # re-seed if vault empty"
echo "  ./dev/scripts/dev-reset.sh    # wipe data + rebootstrap"
echo "  ./dev/scripts/dev-down.sh     # stop stack"
