# TALOS production (kanda-server)

This is the **usable** stack: your password manager on the mini-PC, with the existing Keycloak (NDK) as login. It is **not** the Fedora lab (`start-dev` on `:8080`).

Lab stays on the workstation for hacking. Production does **not** ship a Keycloak container.

## What you get

| Piece | Where |
| --- | --- |
| UI | `https://talos.kanda.cloud` (Apache → `talos-web:3000`) |
| Login | Keycloak `https://dev.kanda.cloud:8483` realm **`talos`** |
| Secrets | GPG bunker on disk under `/docker/volumes-data/talos/` |
| Compose | `docker-compose.prod.yaml` + `.env.prod` |

Two doors, same as lab: Keycloak says who you are; your **vault passphrase** decrypts GPG. `TALOS_CUSTODY_MODE=strict`.

Do **not** put HashiCorp unseal keys in here until you have logged in once and the bunker unlocks. HashiCorp can wait.

## Prerequisites on kanda-server

1. Docker + existing `kanda_net` + Apache LAMP (same as Outline).
2. Wildcard TLS already on Apache (`kanda-cloud.crt` / `.key` / `.ca-bundle`).
3. Keycloak NDK running on `:8483`.
4. A DNS **A** record: host `talos` → `151.237.59.10`. Optional AdGuard rewrite → `10.20.30.40`.
5. This git clone (or the tagged images after `v1.2.0` is pushed to **`kandacloud/talos`**).

Port **3000 on the host** belongs to AdGuard. Production TALOS does not bind it.

## 1. Keycloak realm `talos` (on NDK, not a new container)

Admin console: `https://dev.kanda.cloud:8483` (realm `master` to administer).

1. Create realm **`talos`** (empty; do not import the lab JSON — it contains `dev` / `devpass`).
2. Realm roles: `talos-user`, `talos-admin`.
3. Login theme: copy `dev/keycloak/themes/talos` into NDK themes volume, then Realm settings → Themes → Login = `talos`. If the theme is missing, default login still works.
4. Client **`talos-web`** (confidential):
   - Redirect: `https://talos.kanda.cloud/api/auth/oidc/callback`
   - Web origin: `https://talos.kanda.cloud`
   - Standard flow on. PKCE S256 OK (the web app sends it).
   - Copy the client secret into `.env.prod` `OIDC_CLIENT_SECRET`.
5. Client **`talos-extension`** (public): redirect loopback + `https://*.chromiumapp.org/*`. PKCE S256.
6. Create **your** user (not `dev`). Roles: `talos-user` (and `talos-admin` only if you want operator unseal later). Turn on OTP if you already use Authenticator.

Never use realm `master` for the vault.

## 2. Files on the server

```bash
# clone or pull this repo on kanda
sudo mkdir -p /docker/volumes-data/talos/{web,password-store,bunker-gnupg-users,bunker-wrapped} \
              /docker/volumes-config/talos
sudo chown -R 1000:1000 /docker/volumes-data/talos

cp deploy/kanda/config/storage.json /docker/volumes-config/talos/storage.json
cp .env.prod.example .env.prod
# edit .env.prod: SHARED_SECRET and OIDC_CLIENT_SECRET
chmod 600 .env.prod
```

`SHARED_SECRET`: `openssl rand -hex 32` — HMAC between web/storage/bunker, not your login password.

## 3. Apache vhost

Copy `deploy/kanda/httpd-talos.kanda.cloud.conf` to:

`/docker/volumes-config/lamp/apache2/talos-kanda-cloud.conf`

Add the same bind you use for `docs.kanda.cloud`, then reload `httpd`.

## 4. Start

From the clone directory:

```bash
docker compose -f docker-compose.prod.yaml --env-file .env.prod up --build -d
docker compose -f docker-compose.prod.yaml ps
```

First build on the N100 takes several minutes (Rust). After GitHub tag `v1.2.0` publishes **`kandacloud/talos:{ver}-web|storage|bunker`**, prefer:

```bash
docker compose -f docker-compose.prod.yaml --env-file .env.prod pull
docker compose -f docker-compose.prod.yaml --env-file .env.prod up -d --no-build
```

Bunker health may look **unhealthy** until you unlock a vault. That is normal (`SEALED`).

## 5. First login

1. Open `https://talos.kanda.cloud`
2. Sign in with Keycloak (your new user)
3. Set a **vault passphrase** (GPG). This is not the Keycloak password and not a YubiKey PIN.
4. Create a test secret. Restart `talos-bunker` → UI asks the passphrase again. If that works, you can start moving real logins here.

Browser extension (**talos-vault** on AMO, pending review): server URL `https://talos.kanda.cloud`, issuer `https://dev.kanda.cloud:8483/realms/talos`, client `talos-extension`. Same listing name on Chrome when submitted.

## 6. What this release deliberately skips

- HashiCorp Vault for `.env.prod` (file on disk, like Outline)
- Git backend for the password-store (local volume; add `talos-secrets` later)
- Auto-publish to stores until `PUBLISH_FIREFOX` / `PUBLISH_CHROME` (first AMO listing **talos-vault** is manual, in review)
- Auto-unseal / `convenience` mode
- Shipping `talos-keycloak` `start-dev`

## Rollback

```bash
docker compose -f docker-compose.prod.yaml --env-file .env.prod down
# volumes under /docker/volumes-data/talos stay; do not rm them
```
