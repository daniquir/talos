# TALOS releases

How a version reaches Docker Hub, GitHub Releases, and (optionally) browser stores.

## What a tag does

Pushing `vX.Y.Z` (example: `v1.2.1`) runs [`.github/workflows/release.yml`](../.github/workflows/release.yml):

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
   git tag -a v1.2.1 -m "TALOS v1.2.1"
   git push origin v1.2.1
   ```
5. Watch **Actions → TALOS Release**. When green, images are on Docker Hub.

## Deploy without building from source

```bash
cp -n .env.prod.example .env.prod
# set SHARED_SECRET, OIDC_*, TALOS_VERSION=1.2.1, TALOS_IMAGE=kandacloud/talos
docker compose -f docker-compose.prod.yaml --env-file .env.prod pull
docker compose -f docker-compose.prod.yaml --env-file .env.prod up -d --no-build
```

Pulls:

- `kandacloud/talos:1.2.1-web`
- `kandacloud/talos:1.2.1-storage`
- `kandacloud/talos:1.2.1-bunker`

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
# → dist/talos-extension-v1.2.1.zip
```

## Checklist before `v1.2.1`

- [ ] `main` contains OIDC audience fix + vault UX + storage KEEP/rename fixes
- [ ] `CHANGELOG.md` has `[1.2.1]`
- [ ] Cargo crates + extension manifest at `1.2.1`
- [ ] Keycloak `talos-extension`: Firefox AMO redirect URIs configured on prod
- [ ] Prod `.env.prod`: `TALOS_VERSION=1.2.1` after images publish
- [ ] Optional: `PUBLISH_FIREFOX=true` / `PUBLISH_CHROME=true` for store auto-submit
