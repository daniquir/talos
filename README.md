# TALOS // VAULT-OS

![Build Status](https://img.shields.io/github/actions/workflow/status/daniquir/talos/release.yml?style=flat-square)
![License](https://img.shields.io/github/license/daniquir/talos?style=flat-square)
![Version](https://img.shields.io/github/v/release/daniquir/talos?style=flat-square)

> **Secure Multi-Layered Password Storage System**
> Written in Rust. Dockerized for isolation.

## ✨ Features

*   **Military-Grade Architecture**: 3-layer isolation (Web -> Storage -> Bunker).
*   **Secure Storage**: GPG encryption with RSA 4096-bit keys.
*   **Tree View Navigation**: Hierarchical organization of secrets with categories.
*   **Lazy Loading & Masking**: Secrets are masked by default and only retrieved from the Bunker upon explicit request.
*   **Search & Filter**: Real-time filtering of the secret tree.
*   **Backup & Restore**: Download full encrypted backups as ZIP files and restore them easily.
*   **Git Integration**: Optional automatic versioning and remote backup to a Git repository.
*   **Digital Freeze Mode**: System automatically locks down UI if connection to secure nodes is lost.

## 🏗 Architecture

TALOS implements a strict 3-layer security model inspired by military network segmentation ("Air Gap" simulation).

```mermaid
graph TD
    User((User)) -->|HTTP :3000| Web[Layer 1: talos-web]
    Web -->|net_middleware| Storage[Layer 2: talos-storage]
    Storage -->|net_private| Bunker[Layer 3: talos-bunker]
    
    subgraph Public Network
    Web
    end
    
    subgraph Middleware Network
    Storage
    end
    
    subgraph Isolated Network
    Bunker
    end
```

### 🔌 Network Topology & Hardcoded Hostnames

To ensure integrity and prevent configuration drift, internal communication channels are **hardcoded** into the Docker images. The Docker Compose setup must respect these specific container names for DNS resolution to work.

| Service | Container Name | Internal Port | Hardcoded Upstream URL |
|---------|---------------|---------------|------------------------|
| **Web** | `talos-web` | 3000 | `http://talos-storage:4000` |
| **Storage** | `talos-storage` | 4000 | `http://talos-bunker:5000` |
| **Bunker** | `talos-bunker` | 5000 | *None (Terminal Node)* |

> ⚠️ **CRITICAL:** Do not change the `container_name` or service names in `docker-compose.yaml`. The Rust binaries are compiled expecting these exact hostnames.

## 💾 Storage Backend Configuration

The `talos-storage` service uses a configuration file to define how secrets are stored. Create a `config/storage.json` file in the project root.

### Git Backend (Recommended)
This mode commits and pushes every change to a remote Git repository, providing versioning and off-site backup.

**`config/storage.json`:**
```json
{
  "backend": {
    "type": "git",
    "repository_url": "git@github.com:YOUR_USERNAME/talos-secrets.git",
    "ssh_key_path": "/run/secrets/id_rsa_talos"
  }
}
```
**Setup:**
1. Generate a dedicated SSH key: `ssh-keygen -t rsa -b 4096 -f ~/.ssh/id_rsa_talos -N ""`
2. Add the public key (`~/.ssh/id_rsa_talos.pub`) as a "Deploy Key" with write access in your GitHub repository settings.
3. The `docker-compose.yaml` file already mounts this key into the container.

### Local Backend
This mode stores secrets only in the local Docker volume. You are responsible for backing up this volume.

**`config/storage.json`:**
```json
{
  "backend": { "type": "local" }
}
```

## � Deployment

### Prerequisites
- Docker & Docker Compose
- A GPG ID (email format)

### Quick Start

1. **Configure Environment** (Optional)
   ```bash
   export GPG_ID="your-email@secure.local"
   ```

2. **Launch System**
   ```bash
   docker-compose up --build -d
   ```

3. **Access Interface**
   Open `http://localhost:3000` in your browser.

## 🔐 Security Protocols

### Genesis Protocol (First Run)
On the first startup, `talos-bunker` will detect a missing GPG key and initiate the **Genesis Protocol**:
1. Generates a 4096-bit RSA Master Key.
2. Exports a backup of the private key to a secure location inside the container.
3. **Wait for user retrieval.**

### Emergency Backup Retrieval
To retrieve the generated private key (you only get one chance before you should delete it):

```bash
docker exec -it talos-bunker reveal-backup
```

*Note: The script will display the key and then immediately delete the backup file from the container for security.*

### Managing Secrets

*   **Create Category**: Use the "New Category" button to create folders.
*   **Create Secret**: Use "New Secret" to add entries. You can add metadata like User and URL.
*   **Context Menu**: Right-click on the tree items to Edit or Delete.
*   **Copy Password**: Double-click a secret in the tree or use the copy button in the detail view.

## 🛠 Development

The project is structured as a workspace with three microservices:

- **talos-web**: Axum frontend server serving static assets and proxying requests.
- **talos-storage**: Middleware that manages the filesystem (`~/.password-store`).
- **talos-bunker**: Isolated GPG engine. No internet access.

### Frontend Architecture
The web interface is built with vanilla JavaScript using ES Modules for maintainability:

- `js/app.js`: Main controller and event orchestration.
- `js/api.js`: Data layer handling Fetch requests to the backend.
- `js/ui.js`: DOM manipulation and visual effects.

## 📄 License

This project is licensed under the MIT License - see the LICENSE file for details.
