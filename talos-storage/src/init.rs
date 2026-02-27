use std::env;
use std::fs;
use crate::config::{CONFIG, STORE_PATH};

pub async fn init_storage() {
    let gpg_id = env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string());
    let store_path = STORE_PATH.as_str();
    let gpg_id_file = format!("{}/.gpg-id", store_path);
    let git_dir = format!("{}/.git", store_path);

    // Initialize if gpg-id missing OR git dir missing (repair mode)
    if !std::path::Path::new(&gpg_id_file).exists() || !std::path::Path::new(&git_dir).exists() {
        println!("📦 Initializing/Repairing password vault...");
        fs::create_dir_all(store_path).unwrap();
        
        if !std::path::Path::new(&gpg_id_file).exists() {
            fs::write(gpg_id_file, gpg_id).unwrap();
        }

        if CONFIG.backend.r#type == "git" {
            // Initialize Git repository only if configured
            if !std::path::Path::new(&git_dir).exists() {
                println!("📦 Git repository missing. Initializing...");
                std::process::Command::new("git").args(["config", "--global", "--add", "safe.directory", store_path]).status().unwrap();
                std::process::Command::new("git").args(["init", store_path]).status().unwrap();
                std::process::Command::new("git").args(["-C", store_path, "config", "user.email", "talos@system.local"]).status().unwrap();
                std::process::Command::new("git").args(["-C", store_path, "config", "user.name", "Talos Storage"]).status().unwrap();
            }

            if let (Some(repo_url), Some(ssh_key_path)) = (&CONFIG.backend.repository_url, &CONFIG.backend.ssh_key_path) {
                println!("📦 Configuring Git remote: {}", repo_url);
                std::process::Command::new("git").args(["-C", store_path, "remote", "add", "origin", repo_url]).status().unwrap();

                // Configure SSH
                let ssh_cmd = format!("ssh -i {} -o IdentitiesOnly=yes -o StrictHostKeyChecking=no", ssh_key_path);
                std::process::Command::new("git").args(["-C", store_path, "config", "core.sshCommand", &ssh_cmd]).status().unwrap();

                // Try to pull to check connection and get latest changes
                println!("📦 Performing initial pull from remote...");
                std::process::Command::new("git").args(["-C", store_path, "pull", "origin", "main", "--rebase"]).status(); // Ignore error if main doesn't exist
            } else {
                panic!("'git' backend type requires 'repository_url' and 'ssh_key_path' in config.");
            }
        } else {
            println!("📦 Using LOCAL storage backend.");
            println!("ℹ️  Data location: {}", store_path);
            println!("ℹ️  Backup available via Web UI or manual volume copy.");
        }
    }
}