use std::env;
use std::fs;

pub async fn init_bunker() {
    let gpg_id = env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string());

    // Ensure keys directory exists
    fs::create_dir_all("/root/keys").unwrap();

    // Check status
    let check = std::process::Command::new("gpg").args(["--list-secret-keys", &gpg_id]).output().unwrap();
    
    if !check.status.success() {
        println!("\n[BUNKER] 🛡️ SYSTEM UNINITIALIZED. Waiting for Master Key via Web UI...");
    } else {
        println!("\n[BUNKER] 🔒 SYSTEM READY (Sealed). Waiting for unlock...");
    }
}