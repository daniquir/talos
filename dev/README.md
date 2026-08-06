# Talos development environment

Fast local stack with a known master key, persisted bunker GPG home, and sample secrets for UI/API validation.

## Quick start

```bash
cp -n .env.dev.example .env.dev
./dev/scripts/dev-up.sh
```

Then open http://localhost:3000 and log in with:

| Field | Value |
|-------|-------|
| Master key | `DevMasterKey-ChangeMe!` |
| GPG ID | `dev@talos.local` |

## What you get

- Compose overlay (`docker-compose.dev.yaml`) with `DEBUG=true`
- Isolated volumes under `data-dev/` (does not touch production `data/`)
- Persisted bunker keys (`data-dev/bunker-gnupg`) so restarts only need login/unseal
- HTTP session cookies enabled in DEBUG (required for local `:3000`)
- Sample tree: Personal / Work / Infrastructure (see `fixtures/secrets.json`)
- Fixture private key: `fixtures/master.asc` (DEV ONLY — never use in production)

## Scripts

| Script | Purpose |
|--------|---------|
| `./dev/scripts/dev-up.sh` | Build, start, wait for health, seed |
| `./dev/scripts/dev-seed.sh` | Init/import + login + sample data |
| `./dev/scripts/dev-seed.sh --force` | Seed again even if tree is not empty |
| `./dev/scripts/dev-reset.sh` | Wipe `data-dev` and rebootstrap |
| `./dev/scripts/dev-down.sh` | Stop the stack |

## Manual compose

```bash
docker compose -f docker-compose.yaml -f docker-compose.dev.yaml --env-file .env.dev up --build -d
./dev/scripts/dev-seed.sh
```

## Validation checklist

After `dev-up.sh`:

1. Login with the master key above
2. Tree shows nested categories and secrets
3. Open `Work/Cloud/github` → reveal password
4. Create / edit / delete a secret
5. Restart bunker (`docker restart talos-bunker`) → UI shows sealed → login unseals again
6. Backup / restore from the UI if needed

## Smoke via API

```bash
# after seed (session from login is in the browser; API seed script already checks decrypt)
curl -s http://localhost:3000/api/health
```

## Notes

- Sample passwords in `fixtures/secrets.json` are fake and intended for local testing only.
- On Fedora/SELinux, `dev-up.sh` tries to label `data-dev` for container mounts (`:z` is also set in compose.dev).
- Production deploy remains: `docker compose up --build -d` with your own `.env` and `config/`.
