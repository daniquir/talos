# talos-vault (browser extension)

WebExtension (Chrome / Firefox / Edge, Manifest V3) for your self-hosted TALOS vault.

**Store name:** **talos-vault** — use this on AMO, Chrome Web Store, and Edge (`talos` was taken on Mozilla).  
**Version:** see `manifest.json` (release tags sync it).  
**Gecko id:** `talos@daniquir` (internal; do not change after the first AMO listing).

## Features

- Configurable Talos server URL (Options page) — **HTTPS required** except localhost
- Match / suggest credentials **only while unlocked** (`GET /api/match` requires Bearer)
- Toolbar badge with credential count for the current site (unlocked sessions only)
- Inline form UI on username/email and password fields (including stepped logins)
- **Master key only in the extension popup** — never typed on third-party pages
- Optional: keep Bearer in `storage.session` until the browser closes
- **Auto-lock** after idle (default **15** minutes; 5 / 15 / 30 or off)
- Popup: **Site** matches + **Vault** tree browser
- **Copy** username/password (clipboard cleared after ~45s best-effort), **view/edit** secrets
- Password **generator** in popup editor and Save/Update capture doorhanger
- One-click autofill; keyboard shortcut to fill the primary match (unlocks via popup if locked)
- Context menu: **Fill with talos-vault…** on editable fields
- Content scripts in **all frames**; scans **open shadow roots**
- Save / Update capture after submit or navigation
- Open vault web UI from the popup
- i18n EN/ES shared via `GET/PUT /api/settings`

## Keyboard shortcuts

| Command | Default |
|---------|---------|
| Open talos-vault popup | `Alt+Shift+T` |
| Fill primary match | `Alt+Shift+L` |

Customize in `chrome://extensions/shortcuts` or Firefox Add-ons → Manage extension shortcuts.

## Load for development

### Chrome / Chromium
`chrome://extensions` → Developer mode → **Load unpacked** → `talos-extension/`

### Firefox
`about:debugging#/runtime/this-firefox` → **Load Temporary Add-on…** → `manifest.json`

## Setup

1. Start Talos (e.g. `./dev/scripts/dev-up.sh`) so `http://localhost:3000` is up
2. Open extension **Options**, set Server URL, **Save**, grant host permission
3. Unlock in the **popup** (builds the URL index on the server when the bunker unseals)
4. Use the field icon, context menu, popup, or keyboard shortcut to fill

## Fake login (autofill test)

```bash
./dev/scripts/serve-fake-login.sh
# → http://127.0.0.1:8765/
```

See hub page for form variants. Seed with `./dev/scripts/dev-seed.sh --force`.

## Packaging / store upload

Build a zip (no signing — stores sign on upload or via their tooling):

```bash
./talos-extension/scripts/package.sh
# → dist/talos-extension-v1.0.1.zip
```

### Chrome Web Store
Public name **talos-vault** (same as AMO). First item is **manual**; then GitHub Action `publish-chrome` with `PUBLISH_CHROME=true`.

1. Developer Dashboard → New item → upload the zip (listing name talos-vault)
2. Fill listing (name, screenshots, privacy policy for remote code / host permissions)
3. Publish (unlisted / public)

### Firefox AMO
Listing name **talos-vault** (slug; first upload **pending review** as of 2026-09-22). Do not create a second addon named `talos`.

1. [addons.mozilla.org/developers](https://addons.mozilla.org/developers/) → the existing **talos-vault** listing
2. Later tags: GitHub Action `publish-firefox` with `AMO_JWT_*` secrets (updates this listing; `web-ext sign --channel listed`)
3. For self-distribution / enterprise, use `web-ext sign` with JWT API keys from AMO

Stable gecko id: **`talos@daniquir`** (do not change after the first listing).  
`data_collection_permissions` declares auth + site host + credentials sent only to the user-configured self-hosted Talos/OIDC (required for new AMO submissions since Nov 2025). `strict_min_version` is **140.0** for Firefox built-in data consent.

### Notes for AMO reviewers
- Content scripts on `http(s)://*/*`: detect login forms and autofill on the active page.
- Optional `https://*/*`: connect to the Talos (and Keycloak) origin the user configures — not a vendor cloud.

## Security posture (military-grade defaults)

| Control | Behavior |
|---------|----------|
| Transport | HTTPS required for non-loopback servers; plain HTTP only on localhost / 127.0.0.1 / ::1 |
| Host permissions | No `http://*/*` — only loopback HTTP + `https://*/*` |
| Master key | Accepted only from extension UI (popup). Content scripts cannot unlock |
| Match API | Authenticated (`Bearer` or session). No anonymous vault metadata |
| Badge / suggestions | Only while unlocked — no locked-state match count leak |
| Bearer TTL | **15 minutes** (server-enforced) |
| Auto-lock | Default **15 minutes** idle |
| Session persist | Opt-in; master key never stored |
| Capture stash | Extension `storage.session` only (never page `sessionStorage`) |
| Clipboard | Best-effort clear after copy (~45s / alarm) |
| CSP | Extension pages locked down (`script-src 'self'`, restricted `connect-src`) |
| Multi-user | OIDC (Keycloak) + per-user vault; see [docs/MULTIUSER.md](../docs/MULTIUSER.md) |

### Inherent browser limits (not bugs)

- Autofill must place plaintext into page form fields (the site can read what you fill).
- Extension ↔ Talos still uses HTTPS Bearer + server-side GPG; there is no client-side E2E envelope yet.
- Certificate pinning is not available as a portable MV3 API; rely on system trust store + HTTPS.

## API used

| Endpoint | Purpose |
|----------|---------|
| `POST /api/auth/token` | Unseal + issue Bearer token (15m) — legacy / non-OIDC |
| `POST /api/auth/token/oidc` | OIDC id_token (+ vault key in strict) → Bearer |
| `GET /api/match?host=` | Authenticated URL-index match |
| `POST /api/match/reindex` | Rebuild index |
| `GET /api/tree` | Vault tree |
| `POST /api/save` | Create / update secret |
| `POST /api/decrypt` | Reveal credential |
| `GET /api/settings` | Shared UI language (public) |
| `PUT /api/settings` | Update language (auth required) |
| `POST /api/auth/logout` | Revoke token |
