#!/usr/bin/env bash
# Initialize (if needed), unseal, and load sample secrets into Talos DEV.
# Supports legacy (MULTIUSER=false) and Keycloak multi-user (OIDC + vault passphrase).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

if [[ -f .env.dev ]]; then
  # shellcheck disable=SC1091
  set -a && source .env.dev && set +a
fi

export TALOS_URL="${TALOS_URL:-http://localhost:3000}"
export TALOS_MASTER_KEY="${TALOS_MASTER_KEY:-DevMasterKey-ChangeMe!}"
export TALOS_FIXTURES="${TALOS_FIXTURES:-$ROOT/dev/fixtures/secrets.json}"
export TALOS_MASTER_ASC="${TALOS_MASTER_ASC:-$ROOT/dev/fixtures/master.asc}"
export MULTIUSER="${MULTIUSER:-true}"
export OIDC_ISSUER="${OIDC_ISSUER:-http://localhost:8080/realms/talos}"
export OIDC_CLIENT_ID="${OIDC_CLIENT_ID:-talos-web}"
export OIDC_CLIENT_SECRET="${OIDC_CLIENT_SECRET:-talos-web-dev-secret}"
export KEYCLOAK_ADMIN="${KEYCLOAK_ADMIN:-admin}"
export KEYCLOAK_ADMIN_PASSWORD="${KEYCLOAK_ADMIN_PASSWORD:-admin}"
export KEYCLOAK_USER="${KEYCLOAK_USER:-dev}"
export KEYCLOAK_PASSWORD="${KEYCLOAK_PASSWORD:-devpass}"
FORCE_FLAG="${1:-}"

python3 - "$FORCE_FLAG" <<'PY'
import json, os, sys, time, urllib.error, urllib.parse, urllib.request

base = os.environ["TALOS_URL"].rstrip("/")
master_key = os.environ["TALOS_MASTER_KEY"]
fixtures_path = os.environ["TALOS_FIXTURES"]
master_asc = os.environ["TALOS_MASTER_ASC"]
force = sys.argv[1] == "--force" if len(sys.argv) > 1 else False
multiuser = os.environ.get("MULTIUSER", "true").lower() in ("1", "true", "yes")
issuer = os.environ.get("OIDC_ISSUER", "").rstrip("/")
client_id = os.environ.get("OIDC_CLIENT_ID", "talos-web")
client_secret = os.environ.get("OIDC_CLIENT_SECRET", "")
kc_admin = os.environ.get("KEYCLOAK_ADMIN", "admin")
kc_admin_pass = os.environ.get("KEYCLOAK_ADMIN_PASSWORD", "admin")
kc_user = os.environ.get("KEYCLOAK_USER", "dev")
kc_pass = os.environ.get("KEYCLOAK_PASSWORD", "devpass")

class Session:
    def __init__(self):
        self.cookies = {}
        self.bearer = None

    def request(self, method, path, body=None, timeout=180):
        data = None if body is None else json.dumps(body).encode()
        headers = {}
        if body is not None:
            headers["Content-Type"] = "application/json"
        if self.bearer:
            headers["Authorization"] = f"Bearer {self.bearer}"
        elif self.cookies:
            headers["Cookie"] = "; ".join(f"{k}={v}" for k, v in self.cookies.items())
        req = urllib.request.Request(base + path, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=timeout) as res:
                self._store_cookies(res)
                raw = res.read()
                if not raw:
                    return res.status, None
                try:
                    return res.status, json.loads(raw)
                except json.JSONDecodeError:
                    return res.status, raw.decode()
        except urllib.error.HTTPError as e:
            self._store_cookies(e)
            raw = e.read()
            try:
                payload = json.loads(raw) if raw else None
            except json.JSONDecodeError:
                payload = raw.decode(errors="replace")
            raise RuntimeError(f"{method} {path} -> HTTP {e.code}: {payload}") from e

    def _store_cookies(self, res):
        values = []
        if hasattr(res.headers, "get_all"):
            values = res.headers.get_all("Set-Cookie") or []
        elif "Set-Cookie" in res.headers:
            values = [res.headers["Set-Cookie"]]
        for header in values:
            part = header.split(";", 1)[0]
            if "=" in part:
                k, v = part.split("=", 1)
                self.cookies[k.strip()] = v.strip()

def http_form(url, fields, timeout=30):
    data = urllib.parse.urlencode(fields).encode()
    req = urllib.request.Request(url, data=data, method="POST", headers={
        "Content-Type": "application/x-www-form-urlencoded",
    })
    with urllib.request.urlopen(req, timeout=timeout) as res:
        return json.loads(res.read())

def wait_api(timeout=90):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(base + "/api/health", timeout=3) as res:
                if res.status == 200:
                    return json.loads(res.read())
        except Exception:
            pass
        time.sleep(1)
    raise SystemExit(f"ERROR: API not reachable at {base}")

def wait_keycloak(timeout=90):
    deadline = time.time() + timeout
    url = f"{issuer}/.well-known/openid-configuration"
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=3) as res:
                if res.status == 200:
                    return
        except Exception:
            pass
        time.sleep(1)
    raise SystemExit(f"ERROR: Keycloak not reachable at {issuer}")

def ensure_direct_grants():
    """Enable Resource Owner Password on talos-web (dev seed only)."""
    master = issuer.rsplit("/realms/", 1)[0] + "/realms/master"
    try:
        tok = http_form(f"{master}/protocol/openid-connect/token", {
            "client_id": "admin-cli",
            "username": kc_admin,
            "password": kc_admin_pass,
            "grant_type": "password",
        })
    except Exception as e:
        print(f"    warn: cannot reach Keycloak admin for ROPC enable: {e}")
        return
    admin_token = tok["access_token"]
    realm = issuer.rsplit("/", 1)[-1]
    base_admin = issuer.rsplit("/realms/", 1)[0] + f"/admin/realms/{realm}"
    req = urllib.request.Request(
        f"{base_admin}/clients?clientId={urllib.parse.quote(client_id)}",
        headers={"Authorization": f"Bearer {admin_token}"},
    )
    with urllib.request.urlopen(req, timeout=15) as res:
        clients = json.loads(res.read())
    if not clients:
        print("    warn: talos-web client not found in realm")
        return
    client = clients[0]
    if client.get("directAccessGrantsEnabled"):
        return
    client["directAccessGrantsEnabled"] = True
    cid = client["id"]
    body = json.dumps(client).encode()
    put = urllib.request.Request(
        f"{base_admin}/clients/{cid}",
        data=body,
        method="PUT",
        headers={
            "Authorization": f"Bearer {admin_token}",
            "Content-Type": "application/json",
        },
    )
    with urllib.request.urlopen(put, timeout=15) as res:
        if res.status not in (204, 200):
            raise SystemExit(f"Failed enabling direct grants: HTTP {res.status}")
    print("    enabled directAccessGrants on talos-web (dev)")

def keycloak_password_grant():
    return http_form(f"{issuer}/protocol/openid-connect/token", {
        "client_id": client_id,
        "client_secret": client_secret,
        "username": kc_user,
        "password": kc_pass,
        "grant_type": "password",
        "scope": "openid",
    })

print(f"==> Checking API at {base}")
health = wait_api()
print(f"    health: {health}")

s = Session()
status_code, status = s.request("GET", "/api/auth/status")
print(f"    auth status: {status}")

oidc_on = bool(status.get("oidc_enabled")) or (multiuser and bool(issuer))

if oidc_on:
    print("==> Multi-user / OIDC seed")
    wait_keycloak()
    ensure_direct_grants()
    print(f"==> Keycloak password grant ({kc_user})")
    tokens = keycloak_password_grant()
    id_token = tokens.get("id_token")
    if not id_token:
        raise SystemExit(f"ERROR: no id_token from Keycloak: {list(tokens)}")
    print("==> Issue Talos Bearer (init vault if needed)")
    # Uses TALOS_MASTER_KEY as this user's vault passphrase in local multi-user dev.
    _, tok = s.request("POST", "/api/auth/token/oidc", {
        "id_token": id_token,
        "key": master_key,
    }, timeout=300)
    s.bearer = tok["access_token"]
    print(f"    bearer ok user_sub={tok.get('user_sub')}")
else:
    if not status.get("initialized"):
        print("==> Importing DEV master key (fixture)")
        with open(master_asc) as f:
            private_key = f.read()
        s.request("POST", "/api/initialize/import", {"key": private_key, "passphrase": master_key})
        print("    imported & unsealed")
    else:
        print("==> Already initialized — will login to unseal")

    print("==> Login")
    _, login = s.request("POST", "/api/auth/login", {"key": master_key})
    print(f"    {login}")

_, auth = s.request("GET", "/api/auth/status")
if not auth.get("authenticated") and not s.bearer:
    raise SystemExit(
        f"ERROR: login did not establish a session: {auth}\n"
        "Hint: talos-web must run with DEBUG=true so HTTP cookies work."
    )
print(f"    session ok: authenticated={auth.get('authenticated')} oidc={auth.get('oidc_authenticated')}")

_, tree = s.request("GET", "/api/tree")
if tree and not force:
    print(f"==> Vault already has {len(tree)} root entries — skipping seed (use --force to re-add)")
    print(json.dumps(tree, indent=2)[:1200])
    raise SystemExit(0)

with open(fixtures_path) as f:
    fixtures = json.load(f)

print("==> Creating categories and secrets from fixtures")
for cat in fixtures["categories"]:
    try:
        s.request("POST", "/api/create_category", {"path": cat})
        print(f"  + category {cat}")
    except Exception as e:
        print(f"  ~ category {cat}: {e}")

for secret in fixtures["secrets"]:
    lines = [secret["password"]]
    field_key_map = {
        "user": "User",
        "username": "User",
        "url": "URL",
        "website": "URL",
    }
    for k, v in secret.get("fields", {}).items():
        label = field_key_map.get(k.lower(), k)
        lines.append(f"{label}: {v}")
    content = "\n".join(lines) + "\n"
    s.request("POST", "/api/save", {"path": secret["path"], "content": content})
    print(f"  + secret   {secret['path']}")

_, tree = s.request("GET", "/api/tree")
print(f"==> Seed complete. Root nodes: {len(tree)}")

print("==> Smoke checks")
_, decrypted = s.request("POST", "/api/decrypt", {"path": "Work/Cloud/github", "reveal": True})
first = decrypted.splitlines()[0] if isinstance(decrypted, str) else decrypted
print(f"    decrypt Work/Cloud/github => {first}")
assert "ghp_dev_sample_token_not_real" in str(decrypted), decrypted
if oidc_on:
    print(f"Done. Open {base} → Keycloak ({kc_user}/{kc_pass}) → vault passphrase: {master_key}")
else:
    print(f"Done. Open {base} and login with: {master_key}")
PY
