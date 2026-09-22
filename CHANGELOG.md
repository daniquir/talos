# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

## [1.2.0] - 2026-09-22

Production cut for kanda-server: Keycloak multi-user + browser extension (**talos-vault**). Official Docker image: **`kandacloud/talos`** (`{ver}-web|storage|bunker`). Lab remains `docker-compose.dev.yaml`; production is `docker-compose.prod.yaml` (no bundled `start-dev` Keycloak). See [docs/PRODUCTION.md](docs/PRODUCTION.md) and [docs/RELEASE.md](docs/RELEASE.md).

### Added
- Auth status reports per-user vault initialization (OIDC `sub`), not the legacy shared keyring
- `dev-up.sh` creates/chowns `bunker-gnupg-users` + `bunker-wrapped` as uid 1000 (required for MULTIUSER)
- `dev-seed.sh` supports Keycloak multi-user (password grant + `/api/auth/token/oidc`, auto-init vault)
- Storage vault routes require signed `X-Talos-User-Sub` when `MULTIUSER=true`
- Keycloak login theme `talos` (retro CRT / Fira Code, aligned with vault-os UI)
- **Keycloak multi-user**: OIDC identity (`talos` realm), per-user vaults under `users/{sub}/`, per-user GPG homes in bunker
- Dual-door multi-user auth: Keycloak for identity/`sub` isolation; vault passphrase for GPG decrypt (`strict` default — passphrase is never stored in Keycloak)
- `TALOS_CUSTODY_MODE=strict|convenience` with optional operator KEK unseal + wrapped vault passphrases
- Web: Sign in with Keycloak + vault passphrase unlock; extension OIDC PKCE (`identity`) + `/api/auth/token/oidc`
- HMAC-signed `X-Talos-User-Sub` between web → storage → bunker
- Docs: [docs/MULTIUSER.md](docs/MULTIUSER.md)
- Browser extension MVP (`talos-extension/`): unlock with master key, match by host, one-click autofill (Chrome/Firefox MV3)
- Authenticated `GET /api/match?host=` via plaintext URL index (no passwords; Bearer/session required)
- URL index maintained on save/delete and rebuilt on bunker unlock (`POST /api/match/reindex`)
- Extension toolbar badge with match count while unlocked
- Optional browser-session unlock: Bearer mirrored to `chrome.storage.session` when enabled in extension settings (master key never stored)
- Extension Save/Update capture (Firefox-style submit/navigation) with tree picker for new secrets
- Extension popup Vault tab: browse the full vault tree (search, expand folders, click to autofill)
- Extension v1.0: copy user/password, view/edit secrets (notes), password generator, idle auto-lock (5/15/30m), context menu fill, all-frames + open shadow DOM, keyboard shortcuts, open vault web from popup, packaging script for CWS/AMO
- Extension performance: coalesce/cache MATCH, skip vault origin, debounce badge + MutationObserver, storage-only i18n in content frames
- UI internationalization (English / Spanish) for vault web UI and browser extension
- Shared UI language preference via `GET/PUT /api/settings` (SQLite; web + extension stay in sync)
- `POST /api/auth/token` — issue Bearer API tokens for extension clients (15m TTL, in-memory)
- Dual auth on vault APIs: session cookie **or** `Authorization: Bearer`

### Changed
- Autofill: match/suggest only while unlocked; unlock only in the extension popup (never on web pages)
- Web unlock button shows busy/disabled state while authenticating (parity with extension)
- Web UI uses the extension icon as favicon and header/login logo
- Host matching: only exact host or query-as-subdomain of stored URL (no reverse match)
- Extension Bearer token TTL reduced to **15 minutes**; default idle auto-lock **15 minutes**
- Extension host permissions: removed broad `http://*/*` (HTTPS + loopback HTTP only)
- Session cookies use SameSite=Lax when OIDC redirect is enabled

### Security
- Removed wildcard CORS (extension uses host permissions)
- `GET /api/match` requires authentication (no anonymous vault metadata probing); rate-limited for DoS resistance
- `PUT /api/settings` requires auth; login-screen language stays local until unlock
- Capture no longer writes passwords to page `sessionStorage` (extension session stash only)
- `.talos-url-index.json` gitignored and excluded from git commits/pushes
- Extension refuses non-HTTPS server URLs except localhost / 127.0.0.1 / ::1
- Master key unlock rejected from content scripts; popup-only unlock with pending fill handoff
- Extension page CSP tightened; clipboard clear scheduled after password copy (best-effort)
- Keycloak manages users; vault crypto remains bunker-side (OIDC is not a substitute for GPG custody in `strict` mode)
## [1.1.1] - 2026-08-06

### Added
- Shared secret authentication (`X-Talos-Auth`) on bunker initialize and status checks from Storage
- Local development stack (`docker-compose.dev.yaml`, `dev/scripts`, sample fixtures) for faster UI/API validation

### Changed
- GPG passphrase handling: passphrases are written to a temporary file, zeroized in memory, and removed immediately after use
- GPG trust model set to `always` within the isolated Bunker environment
- UI sizing via root `font-size` (`--ui-scale`) instead of CSS `zoom`/`transform`, so layout, scroll, and overlays stay correct while the interface stays larger
- Session cookies allow non-Secure over HTTP when `DEBUG=true` (local development only)
- Release workflow: pushing a `v*` tag now creates the GitHub Release and then builds/pushes Docker Hub images

### Fixed
- Sidebar vault tree scroll when many categories are expanded and the folder list exceeds the panel height
- Context menu positioning and layout: removed body `scale`/`zoom` that shifted the whole UI and misplaced overlays; aligned menu icons inside each option
- Tree expand arrows and folder/file icons after rem-based UI scaling (replaced broken jsTree sprite with SVG icons)

## [1.1.0] - 2025-04-22
### Security Hardening Release
This release implements comprehensive security improvements following a full security audit.

### Added
- **Rate Limiting**: In-memory rate limiter for authentication endpoints (5 attempts per 60 seconds per IP)
- **CSRF Protection**: Token-based CSRF protection for state-changing operations (save, delete, restore, create, initialize)
- **Mutual Authentication**: HMAC-SHA256 signature verification for inter-service communication (Storage <-> Bunker)
- **Audit Logging**: Comprehensive audit logging across all services (Web, Storage, Bunker) with timestamps and user tracking
- **Integrity Verification**: SHA256 checksum verification for backup/restore operations
- **Memory Security**: Memory zeroization for sensitive data (VAULT_KEY, login credentials) using zeroize crate
- **Session Security**: Enhanced session configuration (HttpOnly, Secure, SameSite=Strict cookies, 2-hour timeout)
- **Request Limits**: 10MB request body size limit to prevent DoS attacks
- **Path Validation**: Comprehensive input validation and sanitization for file paths
- **Shared Secret**: Required SHARED_SECRET environment variable for service authentication
- **Docker Security**: 
  - Non-root user (UID/GID 1000) in all containers
  - Resource limits (CPU, memory) in docker-compose
  - Security hardening (no-new-privileges, read-only root filesystem)
  - Health checks for all services
  - Pinned Alpine versions
- **GPG Security**: Removed --always-trust flag, mandatory GPG_ID variable
- **Input Validation**: Double base64 decode vulnerability fix, proper error handling

### Changed
- **Architecture**: Enhanced 3-layer isolation with mutual authentication and audit trails
- **Dependencies**: Updated to use rustls-tls instead of native TLS
- **Session Management**: Increased session timeout to 2 hours for operational flexibility
- **Error Handling**: Improved error messages without information leakage

### Fixed
- Fixed passphrase file cleanup race condition
- Fixed read-only filesystem blocking database writes
- Fixed non-root user permission issues with mounted volumes
- Fixed certs directory permission issue
- Fixed double base64 decode vulnerability in gpg.rs
- Fixed missing tower-http dependency

## [1.0.0] - 2024-05-22
### Added
- **Tree View**: Hierarchical navigation for secrets with support for categories (folders).
- **Search**: Real-time filtering of the secret tree.
- **Lazy Loading**: Secrets are masked by default ("••••••••••••") and only retrieved from the Bunker when explicitly requested.
- **Clipboard Integration**: One-click copy for passwords, usernames, and URLs.
- **Backup & Restore**: Full system backup to encrypted ZIP and restoration capability.
- **Digital Freeze**: "Winter Mode" that locks the UI if connection to Storage or Bunker is lost.
- **Context Menus**: Custom right-click menus for managing secrets and categories.
- **Git Integration**: Optional backend configuration to sync secrets with a remote Git repository.
- **CI/CD**: Automated Docker image build and push to Docker Hub via GitHub Actions.

### Changed
- **Architecture**: Refined 3-layer isolation (Web -> Storage -> Bunker).
- **UI/UX**: Complete redesign with "Retro Hacking" aesthetic (Tailwind CSS, scanlines, glow effects).
- **Security**: Removed passphrase requirement for viewing (relying on Bunker isolation) and implemented "Pre-flight" checks for operations.

### Fixed
- Fixed issue where creating a secret without a name caused a ghost file.
- Fixed issue where deleting a non-empty category caused a generic error.