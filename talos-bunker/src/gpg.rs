use axum::Json;
use serde::{Deserialize, Serialize};
use std::env;
use std::process::Stdio;
use tokio::process::Command;
use tokio::io::AsyncWriteExt;

#[derive(Deserialize)]
pub struct CryptTask {
    pub payload: String,
    pub mode: String,
}

#[derive(Serialize)]
pub struct CryptResponse {
    pub result: String,
}

fn is_debug() -> bool {
    env::var("DEBUG").unwrap_or_default() == "true"
}

pub async fn process_gpg(Json(req): Json<CryptTask>) -> Json<CryptResponse> {
    let gpg_id = env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string());
    let mut args = vec!["--batch", "--pinentry-mode", "loopback"];
    
    if is_debug() { println!("--> [BUNKER] Processing GPG task: {}", req.mode); }
    
    if req.mode == "check" {
        return Json(CryptResponse { result: "BUNKER_ONLINE".to_string() });
    }

    if req.mode == "decrypt" {
        args.push("-d");
    } else {
        args.extend(["-e", "-r", &gpg_id, "--armor"]); 
    }

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
    
    Json(CryptResponse {
        result: String::from_utf8_lossy(&output.stdout).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::init_bunker;
    use axum::Json;

    #[tokio::test]
    async fn test_encrypt_decrypt_flow() {
        // Ensure GPG key exists for the test
        init_bunker().await;

        let original_payload = "TALOS system check: OK".to_string();

        // 1. Encrypt
        let encrypt_task = CryptTask {
            payload: original_payload.clone(),
            mode: "encrypt".to_string(),
        };
        let Json(encrypt_response) = process_gpg(Json(encrypt_task)).await;
        let encrypted_payload = encrypt_response.result;

        assert!(encrypted_payload.starts_with("-----BEGIN PGP MESSAGE-----"));
        assert!(encrypted_payload.ends_with("-----END PGP MESSAGE-----\n"));

        // 2. Decrypt
        let decrypt_task = CryptTask {
            payload: encrypted_payload,
            mode: "decrypt".to_string(),
        };
        let Json(decrypt_response) = process_gpg(Json(decrypt_task)).await;
        let decrypted_payload = decrypt_response.result.trim().to_string();

        assert_eq!(original_payload, decrypted_payload);
    }

    #[tokio::test]
    async fn test_check_mode() {
        let check_task = CryptTask { payload: "".to_string(), mode: "check".to_string() };
        let Json(check_response) = process_gpg(Json(check_task)).await;
        assert_eq!(check_response.result, "BUNKER_ONLINE");
    }
}