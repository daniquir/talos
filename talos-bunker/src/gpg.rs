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
use base64::{Engine as _, engine::general_purpose};
use zeroize::Zeroize;

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
}

fn is_debug() -> bool {
    env::var("DEBUG").unwrap_or_default() == "true"
}

pub async fn process_gpg(State(_state): State<AppState>, Json(req): Json<CryptTask>) -> Json<CryptResponse> {
    let gpg_id = env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string());
    let mut args = vec!["--batch", "--pinentry-mode", "loopback"];
    
    match req.mode.as_str() {
        "check" => {
            // Check if key exists on disk
            let check = Command::new("gpg")
                .args(["--batch", "--list-secret-keys", &gpg_id])
                .output()
                .await;
                
            let check = match check {
                Ok(o) => o,
                Err(_) => return Json(CryptResponse { result: "ERROR_GPG_NOT_FOUND".to_string() }),
            };
                
            if !check.status.success() {
                 return Json(CryptResponse { result: "UNINITIALIZED".to_string() });
            }

            let is_unsealed = VAULT_KEY.lock().map(|guard| guard.is_some()).unwrap_or(false);
            if is_unsealed {
                Json(CryptResponse { result: "UNSEALED".to_string() })
            } else {
                Json(CryptResponse { result: "SEALED".to_string() })
            }
        },

        "unlock" => {
            if let Ok(mut guard) = VAULT_KEY.lock() {
                *guard = Some(req.payload.into_bytes());
                Json(CryptResponse { result: "VAULT_UNSEALED".to_string() })
            } else {
                Json(CryptResponse { result: "ERROR_LOCK_FAILED".to_string() })
            }
        },

        "initialize" => {
            // Double check it doesn't exist
            let check = Command::new("gpg")
                .args(["--batch", "--list-secret-keys", &gpg_id])
                .output()
                .await;
                
            if let Ok(output) = check {
                if output.status.success() {
                    return Json(CryptResponse { result: "ERROR_ALREADY_INIT".to_string() });
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
                Err(_) => return Json(CryptResponse { result: "ERROR_SPAWN".to_string() }),
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
                    Json(CryptResponse { result: "INITIALIZED".to_string() })
                } else {
                    Json(CryptResponse { result: "ERROR_GEN".to_string() })
                }
            } else {
                Json(CryptResponse { result: "ERROR_WAIT".to_string() })
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
                Err(_) => return Json(CryptResponse { result: "ERROR_SPAWN".to_string() }),
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
                    Json(CryptResponse { result: "INITIALIZED".to_string() })
                } else {
                    Json(CryptResponse { result: "ERROR_IMPORT".to_string() })
                }
            } else {
                Json(CryptResponse { result: "ERROR_WAIT".to_string() })
            }
        },

        "export_key" => {
            let output = Command::new("gpg")
                .args(["--batch", "--export-secret-keys", "--armor", &gpg_id])
                .output()
                .await;
            
            match output {
                Ok(o) => Json(CryptResponse { result: String::from_utf8_lossy(&o.stdout).to_string() }),
                Err(_) => Json(CryptResponse { result: "ERROR_EXPORT".to_string() }),
            }
        },

        "decrypt" | "encrypt" => {
            if req.mode == "decrypt" {
                args.extend(["-d"]);
            } else {
                args.extend(["-e", "-r", &gpg_id, "--armor"]);
            }

            // Retrieve Key from Memory
            let passphrase = match VAULT_KEY.lock() {
                Ok(guard) => match guard.as_ref() {
                    Some(p) => p.clone(),
                    None => return Json(CryptResponse { result: "ERROR_VAULT_SEALED".to_string() }),
                },
                Err(_) => return Json(CryptResponse { result: "ERROR_LOCK_FAILED".to_string() }),
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
                    return Json(CryptResponse { result: "ERROR_SPAWN".to_string() });
                },
            };

            // Write passphrase to GPG via stdin (more secure than file)
            if let Some(mut stdin) = child.stdin.take() {
                if let Err(_) = stdin.write_all(&passphrase).await {
                    let mut p = passphrase;
                    p.zeroize();
                    return Json(CryptResponse { result: "ERROR_WRITE_PASSPHRASE".to_string() });
                }
                // Zeroize passphrase immediately after use
                let mut p = passphrase;
                p.zeroize();
                drop(stdin);
            }

            let output = child.wait_with_output().await;
            
            match output {
                Ok(o) => Json(CryptResponse {
                    result: String::from_utf8_lossy(&o.stdout).to_string(),
                }),
                Err(_) => Json(CryptResponse { result: "ERROR_GPG_EXEC".to_string() }),
            }
        },

        _ => Json(CryptResponse { result: "ERROR_INVALID_MODE".to_string() }),
    }
}
