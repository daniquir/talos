# TALOS releases

How a version reaches Docker Hub, GitHub Releases, and (optionally) browser stores.

## What a tag does

Pushing `vX.Y.Z` (example: `v1.2.4`) runs [`.github/workflows/release.yml`](../.github/workflows/release.yml):

| Step | Output |
|------|--------|
| GitHub Release | Notes from `CHANGELOG.md` + auto notes |
| Docker Hub | **`kandacloud/talos`** with tags `{X.Y.Z}-web\|storage\|bunker` and floating `web\|storage\|bunker` |
| Extension asset | `talos-extension-vX.Y.Z.zip` on the Release |
| Chrome / Firefox | Only if repo **variables** `PUBLISH_CHROME` / `PUBLISH_FIREFOX` are `true` |

Official image name: **`kandacloud/talos`** (single repository). The three layers share that name and differ by tag.

**Store listing name:** **talos-vault** (`talos` was taken on AMO). Use the same name on Chrome and Edge. Manifest gecko id stays `talos@daniquir`.

## Cut a release

1. Merge feature/bugfix work into `main`.
2. Open a `release/X.Y.Z` PR that:
   - Adds `## [X.Y.Z] - YYYY-MM-DD` in `CHANGELOG.md`
   - Sets Cargo crates + extension `manifest.json` (+ compose / docs examples) to `X.Y.Z`
   - Uses commit title **`Release vX.Y.Z: …`** (squash-merge keeps that title on `main`)
3. Merge the release PR (human). **Do not tag by hand.**
4. [`.github/workflows/tag-release.yml`](../.github/workflows/tag-release.yml) creates annotated tag `vX.Y.Z` and **dispatches** [`.github/workflows/release.yml`](../.github/workflows/release.yml) (`GITHUB_TOKEN` cannot trigger other workflows by pushing the tag alone).
5. `release.yml` → Hub / GitHub Release / optional stores. Watch **Actions**.

Private deploy (outside this repo): GitHub **Settings → Webhooks** on Release events → your deploy hook; the worker waits for Hub tags.

## Deploy without building from source

```bash
cp -n .env.prod.example .env.prod
# set SHARED_SECRET, OIDC_*, TALOS_VERSION=1.2.4, TALOS_IMAGE=kandacloud/talos
docker compose -f docker-compose.prod.yaml --env-file .env.prod pull
docker compose -f docker-compose.prod.yaml --env-file .env.prod up -d --no-build
```

Pulls:

- `kandacloud/talos:1.2.4-web`
- `kandacloud/talos:1.2.4-storage`
- `kandacloud/talos:1.2.4-bunker`

## GitHub secrets & variables

### Required (Docker)

| Secret | Purpose |
|--------|---------|
| `DOCKER_USERNAME` | Docker Hub user that can push to org **`kandacloud`** |
| `DOCKER_PASSWORD` | Access token (preferred) or password |

| Variable | Default | Purpose |
|----------|---------|---------|
| `DOCKER_IMAGE` | `kandacloud/talos` | Full Hub image name |

Create the Hub repo once (public): **`kandacloud/talos`**.

### Optional (Chrome Web Store)

| Name | Type |
|------|------|
| `PUBLISH_CHROME` | Variable = `true` to enable |
| `CHROME_EXTENSION_ID` | Secret |
| `CHROME_CLIENT_ID` | Secret (Google Cloud OAuth client) |
| `CHROME_CLIENT_SECRET` | Secret |
| `CHROME_REFRESH_TOKEN` | Secret |

### Optional (Firefox AMO)

| Name | Type |
|------|------|
| `PUBLISH_FIREFOX` | Variable = `true` to enable |
| `AMO_JWT_ISSUER` | Secret (API key “JWT issuer”) |
| `AMO_JWT_SECRET` | Secret |
| `AMO_CHANNEL` | Variable: `listed` (default) or `unlisted` |

Addon id: **`talos@daniquir`**. Public store name: **`talos-vault`**.

AMO listing **talos-vault** is approved; enable `PUBLISH_FIREFOX` when you want tag pushes to submit updates for review automatically.

## Local extension zip (manual upload)

```bash
./talos-extension/scripts/package.sh
# → dist/talos-extension-v1.2.4.zip
```

## Checklist before merging `release/1.2.4`

- [ ] `main` contains the fixes for this release
- [ ] `CHANGELOG.md` has `[1.2.4]`
- [ ] Cargo crates + extension manifest at `1.2.4`
- [ ] Commit title is `Release v1.2.4: …` (auto-tag on merge)
- [ ] Keycloak `talos-extension`: Firefox loopback `http://127.0.0.1/mozoauth2/<hash>/` (and/or `http://127.0.0.1/*`) on prod
- [ ] After Hub publish: prod Vault / `.env.prod` `TALOS_VERSION=1.2.4` (or deploy webhook)
- [ ] Optional: `PUBLISH_FIREFOX=true` / `PUBLISH_CHROME=true` for store auto-submit
