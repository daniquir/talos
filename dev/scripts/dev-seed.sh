#!/usr/bin/env bash
# Initialize (if needed), unseal, and load sample secrets into Talos DEV.
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
FORCE_FLAG="${1:-}"

python3 - "$FORCE_FLAG" <<'PY'
import json, os, sys, time, urllib.error, urllib.request

base = os.environ["TALOS_URL"].rstrip("/")
master_key = os.environ["TALOS_MASTER_KEY"]
fixtures_path = os.environ["TALOS_FIXTURES"]
master_asc = os.environ["TALOS_MASTER_ASC"]
force = sys.argv[1] == "--force" if len(sys.argv) > 1 else False

class Session:
    def __init__(self):
        self.cookies = {}

    def request(self, method, path, body=None, timeout=120):
        data = None if body is None else json.dumps(body).encode()
        headers = {}
        if body is not None:
            headers["Content-Type"] = "application/json"
        if self.cookies:
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
        # HTTPMessage may expose get_all; fall back to get_all via headers API
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

print(f"==> Checking API at {base}")
health = wait_api()
print(f"    health: {health}")

s = Session()
status_code, status = s.request("GET", "/api/auth/status")
print(f"    auth status: {status}")

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
if "id" not in s.cookies and not s.cookies:
    # tower-sessions default cookie name is usually "id"
    print(f"    cookies: {list(s.cookies)}")

_, auth = s.request("GET", "/api/auth/status")
if not auth.get("authenticated"):
    raise SystemExit(
        f"ERROR: login did not establish a session: {auth}\n"
        "Hint: talos-web must run with DEBUG=true so HTTP cookies work."
    )
print(f"    session ok: {auth}")

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
    for k, v in secret.get("fields", {}).items():
        lines.append(f"{k}: {v}")
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
print(f"Done. Open {base} and login with: {master_key}")
PY
