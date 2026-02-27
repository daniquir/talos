# Changelog

All notable changes to this project will be documented in this file.

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