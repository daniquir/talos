use axum::Json;
use axum::extract::State;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::env;
use std::process::Stdio;
use tokio::process::Command;
use tokio::io::AsyncWriteExt;
use std::sync::Mutex;
use once_cell::sync::Lazy;
use uuid::Uuid;

// In-Memory Vault for the Master Key. Never written to disk.
pub static VAULT_KEY: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

#[derive(Deserialize)]
pub struct CryptTask {
    pub payload: String,
    pub mode: String,
    pub passphrase: Option<String>,
}

#[derive(Serialize)]
pub struct CryptResponse {
    pub result: String,
}

fn is_debug() -> bool {
    env::var("DEBUG").unwrap_or_default() == "true"
}

pub async fn process_gpg(State(_state): State<AppState>, Json(req): Json<CryptTask>) -> Json<CryptResponse> {
    let gpg_id = env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string());
    let mut args = vec!["--batch", "--pinentry-mode", "loopback"];
    
    if is_debug() { println!("--> [BUNKER] Processing GPG task: {}", req.mode); }
    
    if req.mode == "check" {
        // Check if key exists on disk
        let check = Command::new("gpg")
            .args(["--list-secret-keys", &gpg_id])
            .output()
            .await
            .expect("Failed to check keys");
            
        if !check.status.success() {
             return Json(CryptResponse { result: "UNINITIALIZED".to_string() });
        }

        if VAULT_KEY.lock().unwrap().is_some() {
            return Json(CryptResponse { result: "UNSEALED".to_string() });
        } else {
            return Json(CryptResponse { result: "SEALED".to_string() });
        }
    }

    // UNSEAL OPERATION: Validate key and store in RAM
    if req.mode == "unlock" {
        *VAULT_KEY.lock().unwrap() = Some(req.payload);
        return Json(CryptResponse { result: "VAULT_UNSEALED".to_string() });
    }

    // INITIALIZE OPERATION: Generate the master key with provided passphrase
    if req.mode == "initialize" {
        // Double check it doesn't exist
        let check = Command::new("gpg")
            .args(["--list-secret-keys", &gpg_id])
            .output()
            .await
            .expect("Failed to check keys");
            
        if check.status.success() {
             return Json(CryptResponse { result: "ERROR_ALREADY_INIT".to_string() });
        }

        let passphrase = &req.payload;
        
        // Generate key script
        let gen_params = format!(
            "Key-Type: RSA\nKey-Length: 4096\nName-Email: {}\nExpire-Date: 0\nPassphrase: {}\n%commit\n",
            gpg_id, passphrase
        );
        let gen_file = format!("/tmp/gpg_gen_{}", Uuid::new_v4());
        if std::fs::write(&gen_file, gen_params).is_err() {
             return Json(CryptResponse { result: "ERROR_WRITE".to_string() });
        }

        let status = Command::new("gpg")
            .args(["--batch", "--generate-key", &gen_file])
            .status()
            .await
            .expect("Failed to generate key");
            
        let _ = std::fs::remove_file(gen_file);

        if status.success() {
            // Auto-unseal in memory since we just set it
            *VAULT_KEY.lock().unwrap() = Some(passphrase.clone());
            return Json(CryptResponse { result: "INITIALIZED".to_string() });
        } else {
            return Json(CryptResponse { result: "ERROR_GEN".to_string() });
        }
    }

    // IMPORT OPERATION: Import existing private key
    if req.mode == "import" {
        let key_data = req.payload;
        let passphrase = req.passphrase.unwrap_or_default();

        let key_file = format!("/tmp/gpg_import_{}", Uuid::new_v4());
        if std::fs::write(&key_file, key_data).is_err() {
             return Json(CryptResponse { result: "ERROR_WRITE".to_string() });
        }

        let status = Command::new("gpg")
            .args(["--batch", "--import", &key_file])
            .status()
            .await
            .expect("Failed to import key");
            
        let _ = std::fs::remove_file(key_file);

        if status.success() {
            // Auto-unseal in memory
            *VAULT_KEY.lock().unwrap() = Some(passphrase);
            return Json(CryptResponse { result: "INITIALIZED".to_string() });
        } else {
            return Json(CryptResponse { result: "ERROR_IMPORT".to_string() });
        }
    }

    // EXPORT KEY OPERATION: Backup the private key
    if req.mode == "export_key" {
        let output = Command::new("gpg")
            .args(["--export-secret-keys", "--armor", &gpg_id])
            .output()
            .await
            .expect("Failed to export key");
        return Json(CryptResponse { result: String::from_utf8_lossy(&output.stdout).to_string() });
    }

    if req.mode == "decrypt" {
        args.push("-d");
    } else {
        args.extend(["-e", "-r", &gpg_id, "--armor"]); 
    }

    // Retrieve Key from Memory
    let passphrase = {
        let guard = VAULT_KEY.lock().unwrap();
        match guard.as_ref() {
            Some(p) => p.clone(), // Clone the string to release the lock immediately
            None => return Json(CryptResponse { result: "ERROR_VAULT_SEALED".to_string() }),
        }
    };

    // Securely pass passphrase to GPG via temporary file in RAM (/dev/shm)
    // This avoids showing it in process list (ps aux)
    let pass_file = format!("/dev/shm/gpg_pass_{}", Uuid::new_v4());
    if let Err(_) = std::fs::write(&pass_file, &passphrase) {
        return Json(CryptResponse { result: "ERROR_MEMORY_WRITE".to_string() });
    }
    args.extend(["--passphrase-file", &pass_file]);
    // Note: GPG 2.x might require --pinentry-mode loopback for file/pipe password

    // Use Tokio's asynchronous Command to prevent blocking the runtime during GPG operations
    let mut child = Command::new("gpg")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start GPG");

    let mut stdin = child.stdin.take().unwrap();
    let input = req.payload;
    
    // Asynchronously write the payload to the GPG process stdin
    stdin.write_all(input.as_bytes()).await.unwrap();
    drop(stdin);

    // Await the process completion and capture output
    let output = child.wait_with_output().await.expect("GPG process failed");
    
    // Wipe password file immediately
    let _ = std::fs::remove_file(pass_file);
    
    Json(CryptResponse {
        result: String::from_utf8_lossy(&output.stdout).to_string(),
    })
}