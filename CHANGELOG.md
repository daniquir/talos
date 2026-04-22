# Changelog

All notable changes to this project will be documented in this file.

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