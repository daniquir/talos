# TALOS releases

How a version reaches Docker Hub, GitHub Releases, and (optionally) browser stores.

## What a tag does

Pushing `vX.Y.Z` (example: `v1.2.0`) runs [`.github/workflows/release.yml`](../.github/workflows/release.yml):

| Step | Output |
|------|--------|
| GitHub Release | Notes from `CHANGELOG.md` + auto notes |
| Docker Hub | **`kandacloud/talos`** with tags `{X.Y.Z}-web\|storage\|bunker` and floating `web\|storage\|bunker` |
| Extension asset | `talos-extension-vX.Y.Z.zip` on the Release |
| Chrome / Firefox | Only if repo **variables** `PUBLISH_CHROME` / `PUBLISH_FIREFOX` are `true` |

Official image name: **`kandacloud/talos`** (single repository). The three layers share that name and differ by tag.

**Store listing name:** **talos-vault** (`talos` was taken on AMO). Use the same name on Chrome and Edge. Manifest gecko id stays `talos@daniquir`.

## Cut a release

1. Merge the work into `main`.
2. Ensure `CHANGELOG.md` has a `## [X.Y.Z] - YYYY-MM-DD` section (not only Unreleased).
3. Align Cargo crate versions if needed (`talos-*/Cargo.toml`).
4. Tag and push:
   ```bash
   git checkout main
   git pull
   git tag -a v1.2.0 -m "TALOS v1.2.0"
   git push origin v1.2.0
   ```
5. Watch **Actions → TALOS Release**. When green, images are on Docker Hub.

## Deploy without building from source

```bash
cp -n .env.prod.example .env.prod
# set SHARED_SECRET, OIDC_*, TALOS_VERSION=1.2.0, TALOS_IMAGE=kandacloud/talos
docker compose -f docker-compose.prod.yaml --env-file .env.prod pull
docker compose -f docker-compose.prod.yaml --env-file .env.prod up -d --no-build
```

Pulls:

- `kandacloud/talos:1.2.0-web`
- `kandacloud/talos:1.2.0-storage`
- `kandacloud/talos:1.2.0-bunker`

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

While the first AMO review for **talos-vault** is pending, leave `PUBLISH_FIREFOX` unset/`false`.

## Local extension zip (manual upload)

```bash
./talos-extension/scripts/package.sh
# → dist/talos-extension-v*.zip
```

## Checklist before `v1.2.0`

- [ ] `main` contains multi-user + extension + theme
- [ ] `CHANGELOG.md` has `[1.2.0]`
- [ ] Docker Hub: `DOCKER_*` secrets; repo **`kandacloud/talos`**
- [ ] Prod `.env.prod`: `TALOS_IMAGE=kandacloud/talos`
- [ ] AMO first version: **talos-vault** (in review) — then enable `PUBLISH_FIREFOX`
- [ ] CWS first version: name **talos-vault**, then enable `PUBLISH_CHROME`
