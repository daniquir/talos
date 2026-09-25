# Multi-user Talos + Keycloak

Talos supports **per-user vaults** keyed by Keycloak `sub`. Identity and cryptography stay separate (**two doors**):

1. **Keycloak (OIDC)** — proves *who you are* and scopes data to your `sub` (your tree, your GPG home).
2. **Vault passphrase** — the GPG secret that *decrypts* your vault. Never stored in Keycloak.

Product default: **`TALOS_CUSTODY_MODE=strict`** (both steps every session). Do not put the vault passphrase in Keycloak user attributes.

Production (kanda-server, Keycloak NDK, no `start-dev`): [PRODUCTION.md](PRODUCTION.md).

## Modes

| Env | Meaning |
|-----|---------|
| `MULTIUSER=true` | Per-user storage under `users/{sub}/` and per-user GPG homes. OIDC required when `OIDC_ISSUER` is set. |
| `MULTIUSER=false` | Legacy single vault (master key only). |
| `TALOS_CUSTODY_MODE=strict` (**default / recommended**) | After OIDC, user must enter their **vault passphrase** each session. |
| `TALOS_CUSTODY_MODE=convenience` | Optional softer mode: operator KEK unwraps a server-side wrapped passphrase after OIDC. Higher host trust; not the military baseline. |

**Trust note:** `convenience` means a host that holds an unsealed KEK can unwrap user vault keys. Prefer `strict`.

## Themes

Login UI uses the custom **talos** Keycloak theme (`dev/keycloak/themes/talos/`) — black / green CRT aesthetic matching the vault web UI.

If the realm was imported before the theme existed, set **Realm settings → Themes → Login theme = talos** (or re-import / recreate the realm).

## First login (strict custody)

1. **Sign in with Keycloak** (`dev` / `devpass` in local stack).
2. If your per-user vault is empty, Talos opens **vault setup** — generate or import a GPG passphrase. This is **not** the legacy `TALOS_MASTER_KEY` / `DevMasterKey-ChangeMe!` (that key only unlocks the old single-tenant keyring).
3. Later sessions: Keycloak → unlock with **your** vault passphrase.

Ensure `data-dev/bunker-gnupg-users` is owned by uid `1000` (`./dev/scripts/dev-up.sh` does this). If that directory is root-owned, per-user init/unlock fails with permission errors.

## Dev stack

```bash
./dev/scripts/dev-up.sh
# Keycloak: http://localhost:8080  (admin / admin)
# Realm: talos
# User:  dev / devpass  (roles: talos-user, talos-admin)
# Web:   http://localhost:3000
```

1. Open Talos → **Sign in with Keycloak**.
2. Create / unlock vault with a passphrase (your personal GPG passphrase).
3. Extension: set Server URL + OIDC issuer `http://localhost:8080/realms/talos`, client `talos-extension`. Unlock runs Keycloak PKCE then vault passphrase (strict).
4. Production Firefox (AMO): Keycloak Valid redirect URIs must include `https://*.extensions.allizom.org/*` (or the exact identity redirect). Server accepts id_token audience `talos-extension` as well as `talos-web` (`OIDC_AUDIENCES`).

### Operator unseal (convenience only)

```bash
# Env on bunker (optional boot):
TALOS_OPERATOR_KEK=DevOperatorKek-ChangeMe!

# Or API (requires talos-admin after OIDC):
curl -X POST http://localhost:3000/api/auth/operator/unseal \
  -H 'Content-Type: application/json' \
  -d '{"key":"DevOperatorKek-ChangeMe!"}' \
  --cookie '...' 
```

## Layout

- Storage: `$PASSWORD_STORE_DIR/users/{sanitized_sub}/…`
- Bunker GPG: `$GNUPG_USERS_DIR/{sanitized_sub}/`
- Wrapped keys (convenience): `$WRAPPED_KEYS_DIR/{sub}.wrap`
- Internal identity: `X-Talos-User-Sub` + HMAC `X-Talos-User-Sig` (SHARED_SECRET)

## Migration from single-tenant

1. Set `MULTIUSER=false` until you are ready, **or**
2. Move existing password-store contents into `users/<bootstrap_sub>/` and import/re-init that user’s GPG keyring under the matching gnupg home.
3. Point users at Keycloak; each first login initializes their empty vault if uninitialized.

There is no automatic rewrite of a shared GPG key into N user keys — treat migration as a planned cutover.
