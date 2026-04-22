use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::env;
use std::process::Stdio;
use tokio::process::Command;
use tokio::io::AsyncWriteExt;
use std::sync::Mutex;
use once_cell::sync::Lazy;
use uuid::Uuid;
use base64::{Engine as _, engine::general_purpose};
use zeroize::Zeroize;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use hex;

type HmacSha256 = Hmac<Sha256>;
// In-Memory Vault for the Master Key. Never written to disk.
pub static VAULT_KEY: Lazy<Mutex<Option<Vec<u8>>>> = Lazy::new(|| Mutex::new(None));

#[derive(Deserialize)]
pub struct CryptTask {
    pub payload: String,
    pub mode: String,
    pub passphrase: Option<String>,
    pub key_type: Option<String>,
}

#[derive(Serialize)]
pub struct CryptResponse {
    pub result: String,
    pub signature: Option<String>,
}

fn is_debug() -> bool {
    env::var("DEBUG").unwrap_or_default() == "true"
}

fn log_audit_event(action: &str, status: &str, details: &str) {
    let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    eprintln!("[AUDIT {}] ACTION={} STATUS={} DETAILS={}", timestamp, action, status, details);
}

fn sign_response(result: &str) -> String {
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    let mut mac = HmacSha256::new_from_slice(shared_secret.as_bytes()).unwrap();
    mac.update(result.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());
    signature
}

pub async fn process_gpg(State(_state): State<AppState>, headers: HeaderMap, Json(req): Json<CryptTask>) -> Json<CryptResponse> {
    // Verify shared secret for authentication
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    if let Some(auth_header) = headers.get("X-Talos-Auth") {
        if auth_header.to_str().unwrap_or("") != shared_secret {
            log_audit_event("auth", "failed", "invalid shared secret");
            return Json(CryptResponse { result: "ERROR_UNAUTHORIZED".to_string(), signature: None });
        }
    } else {
        log_audit_event("auth", "failed", "missing auth header");
        return Json(CryptResponse { result: "ERROR_UNAUTHORIZED".to_string(), signature: None });
    }
    
    let gpg_id = env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string());
    let mut args = vec!["--batch", "--pinentry-mode", "loopback"];
    
    match req.mode.as_str() {
        "check" => {
            log_audit_event("gpg_check", "started", &format!("checking key for {}", gpg_id));
            
            // Check if key exists on disk
            let check = Command::new("gpg")
                .args(["--batch", "--list-secret-keys", &gpg_id])
                .output()
                .await;
                
            let check = match check {
                Ok(o) => o,
                Err(_) => {
                    log_audit_event("gpg_check", "failed", "GPG not found");
                    return Json(CryptResponse { result: "ERROR_GPG_NOT_FOUND".to_string(), signature: None });
                },
            };
                
            if !check.status.success() {
                 return Json(CryptResponse { result: "UNINITIALIZED".to_string(), signature: None });
            }

            let is_unsealed = VAULT_KEY.lock().map(|guard| guard.is_some()).unwrap_or(false);
            if is_unsealed {
                let result = "UNSEALED".to_string();
                let signature = sign_response(&result);
                Json(CryptResponse { result, signature: Some(signature) })
            } else {
                let result = "SEALED".to_string();
                let signature = sign_response(&result);
                Json(CryptResponse { result, signature: Some(signature) })
            }
        },

        "unlock" => {
            log_audit_event("vault_unlock", "attempted", "unlocking memory vault");
            
            if let Ok(mut guard) = VAULT_KEY.lock() {
                *guard = Some(req.payload.into_bytes());
                log_audit_event("vault_unlock", "success", "memory vault unlocked");
                let result = "VAULT_UNSEALED".to_string();
                let signature = sign_response(&result);
                Json(CryptResponse { result, signature: Some(signature) })
            } else {
                log_audit_event("vault_unlock", "failed", "lock acquisition failed");
                Json(CryptResponse { result: "ERROR_LOCK_FAILED".to_string(), signature: None })
            }
        },

        "initialize" => {
            log_audit_event("gpg_init", "started", &format!("initializing key for {}", gpg_id));
            
            // Double check it doesn't exist
            let check = Command::new("gpg")
                .args(["--batch", "--list-secret-keys", &gpg_id])
                .output()
                .await;
                
            if let Ok(output) = check {
                if output.status.success() {
                    return Json(CryptResponse { result: "ERROR_ALREADY_INIT".to_string(), signature: None });
                }
            }

            let passphrase = &req.payload;
            let key_type = req.key_type.as_deref().unwrap_or("RSA");
            
            // Generate key script based on key type
            let gen_params = if key_type == "ed25519" {
                format!(
                    "Key-Type: ED25519\nKey-Curve: 25519\nName-Email: {}\nExpire-Date: 0\nPassphrase: {}\n%commit\n",
                    gpg_id, passphrase
                )
            } else {
                format!(
                    "Key-Type: RSA\nKey-Length: 4096\nName-Email: {}\nExpire-Date: 0\nPassphrase: {}\n%commit\n",
                    gpg_id, passphrase
                )
            };
            
            let child = Command::new("gpg")
                .args(["--batch", "--generate-key"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();

            let mut child = match child {
                Ok(c) => c,
                Err(_) => return Json(CryptResponse { result: "ERROR_SPAWN".to_string(), signature: None }),
            };

            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(gen_params.as_bytes()).await;
                drop(stdin);
            }

            let status = child.wait().await;
            
            if let Ok(s) = status {
                if s.success() {
                    // Set trust using interactive mode, redirect output to avoid interfering with JSON response
                    let trust_script = format!(
                        "echo -e \"trust\\n5\\ny\\n\" | gpg --batch --command-fd 0 --edit-key {} >/dev/null 2>&1",
                        gpg_id
                    );
                    let _ = Command::new("sh")
                        .args(["-c", &trust_script])
                        .status()
                        .await;
                    
                    if let Ok(mut guard) = VAULT_KEY.lock() {
                        *guard = Some(passphrase.clone().into_bytes());
                    }
                    Json(CryptResponse { result: "INITIALIZED".to_string(), signature: None })
                } else {
                    Json(CryptResponse { result: "ERROR_GEN".to_string(), signature: None })
                }
            } else {
                Json(CryptResponse { result: "ERROR_WAIT".to_string(), signature: None })
            }
        },

        "import" => {
            let key_data = req.payload;
            let passphrase = req.passphrase.unwrap_or_default();

            let child = Command::new("gpg")
                .args(["--batch", "--import"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();

            let mut child = match child {
                Ok(c) => c,
                Err(_) => return Json(CryptResponse { result: "ERROR_SPAWN".to_string(), signature: None }),
            };

            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(key_data.as_bytes()).await;
                drop(stdin);
            }

            let status = child.wait().await;
            
            if let Ok(s) = status {
                if s.success() {
                    if let Ok(mut guard) = VAULT_KEY.lock() {
                        *guard = Some(passphrase.into_bytes());
                    }
                    Json(CryptResponse { result: "INITIALIZED".to_string(), signature: None })
                } else {
                    Json(CryptResponse { result: "ERROR_IMPORT".to_string(), signature: None })
                }
            } else {
                Json(CryptResponse { result: "ERROR_WAIT".to_string(), signature: None })
            }
        },

        "export_key" => {
            let output = Command::new("gpg")
                .args(["--batch", "--export-secret-keys", "--armor", &gpg_id])
                .output()
                .await;
            
            return match output {
                Ok(o) => Json(CryptResponse { result: String::from_utf8_lossy(&o.stdout).to_string(), signature: None }),
                Err(_) => Json(CryptResponse { result: "ERROR_EXPORT".to_string(), signature: None }),
            }
        },

        "decrypt" | "encrypt" => {
            log_audit_event(&format!("gpg_{}", req.mode), "started", &format!("operation for {}", gpg_id));
            if req.mode == "decrypt" {
                args.extend(["-d"]);
            } else {
                args.extend(["-e", "-r", &gpg_id, "--armor"]);
            }

            // Retrieve Key from Memory
            let passphrase = match VAULT_KEY.lock() {
                Ok(guard) => match guard.as_ref() {
                    Some(p) => p.clone(),
                    None => return Json(CryptResponse { result: "ERROR_VAULT_SEALED".to_string(), signature: None }),
                },
                Err(_) => return Json(CryptResponse { result: "ERROR_LOCK_FAILED".to_string(), signature: None }),
            };

            let mut final_args = vec!["--batch", "--pinentry-mode", "loopback"];
            if req.mode == "decrypt" {
                final_args.push("-d");
            } else {
                final_args.extend(["-e", "-r", &gpg_id, "--armor"]);
            }

            let input = req.payload;

            // Decode base64 if the payload is base64 encoded (from storage)
            let _decoded_input = match general_purpose::STANDARD.decode(&input) {
                Ok(decoded) => decoded,
                Err(_) => input.into_bytes(),
            };

            // Spawn GPG with stdin piped
            let child = Command::new("gpg")
                .args(final_args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            let mut child = match child {
                Ok(c) => c,
                Err(_) => {
                    // Zeroize passphrase before returning
                    let mut p = passphrase;
                    p.zeroize();
                    return Json(CryptResponse { result: "ERROR_SPAWN".to_string(), signature: None });
                },
            };

            // Write passphrase to GPG via stdin (more secure than file)
            if let Some(mut stdin) = child.stdin.take() {
                if let Err(_) = stdin.write_all(&passphrase).await {
                    let mut p = passphrase;
                    p.zeroize();
                    return Json(CryptResponse { result: "ERROR_WRITE_PASSPHRASE".to_string(), signature: None });
                }
                // Zeroize passphrase immediately after use
                let mut p = passphrase;
                p.zeroize();
                drop(stdin);
            }

            let output = child.wait_with_output().await;
            
            match output {
                Ok(o) => {
                    log_audit_event(&format!("gpg_{}", req.mode), "success", "operation completed");
                    let result = String::from_utf8_lossy(&o.stdout).to_string();
                    let signature = sign_response(&result);
                    Json(CryptResponse { result, signature: Some(signature) })
                },
                Err(e) => {
                    log_audit_event(&format!("gpg_{}", req.mode), "failed", &format!("error: {}", e));
                    Json(CryptResponse { result: "ERROR_GPG_EXEC".to_string(), signature: None })
                },
            }
        },

        _ => Json(CryptResponse { result: "ERROR_INVALID_MODE".to_string(), signature: None }),
    }
}
