use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use crate::AppState;
use crate::user_vault::{
    clear_vault_key, ensure_gnupg_home, get_vault_key, gpg_id_for, has_wrapped_key, is_convenience,
    is_unsealed, multiuser_enabled, operator_sealed, operator_unseal, set_vault_key,
    unwrap_passphrase, vault_id, wrap_and_store_passphrase,
};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tokio::io::AsyncWriteExt;
use base64::{Engine as _, engine::general_purpose};
use zeroize::Zeroize;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use hex;

type HmacSha256 = Hmac<Sha256>;

#[derive(Deserialize)]
pub struct CryptTask {
    pub payload: String,
    pub mode: String,
    pub passphrase: Option<String>,
    pub key_type: Option<String>,
    /// Keycloak subject (or stable user id). Empty / absent = legacy single-tenant.
    #[serde(default)]
    pub user_sub: Option<String>,
}

#[derive(Serialize)]
pub struct CryptResponse {
    pub result: String,
    pub signature: Option<String>,
}

fn log_audit_event(action: &str, status: &str, details: &str) {
    let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    eprintln!("[AUDIT {}] ACTION={} STATUS={} DETAILS={}", timestamp, action, status, details);
}

fn sign_response(result: &str) -> String {
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    let mut mac = HmacSha256::new_from_slice(shared_secret.as_bytes()).unwrap();
    mac.update(result.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn authorize(headers: &HeaderMap) -> bool {
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    match headers.get("X-Talos-Auth") {
        Some(auth_header) => {
            let provided = auth_header.to_str().unwrap_or("");
            ct_eq(provided, &shared_secret)
        }
        None => false,
    }
}

fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn sub_ref(req: &CryptTask) -> Option<&str> {
    req.user_sub.as_deref().filter(|s| !s.is_empty())
}

fn apply_gnupg_env(cmd: &mut Command, home: &Option<PathBuf>) {
    if let Some(h) = home {
        cmd.env("GNUPGHOME", h);
    }
}

async fn gpg_list_secret(gpg_id: &str, home: &Option<PathBuf>) -> Result<bool, ()> {
    let mut cmd = Command::new("gpg");
    apply_gnupg_env(&mut cmd, home);
    let check = cmd
        .args(["--batch", "--list-secret-keys", gpg_id])
        .output()
        .await
        .map_err(|_| ())?;
    Ok(check.status.success())
}

pub async fn process_gpg(State(_state): State<AppState>, headers: HeaderMap, Json(req): Json<CryptTask>) -> Json<CryptResponse> {
    if !authorize(&headers) {
        log_audit_event("auth", "failed", "invalid or missing shared secret");
        return Json(CryptResponse { result: "ERROR_UNAUTHORIZED".to_string(), signature: None });
    }

    let user_owned = req.user_sub.clone().filter(|s| !s.is_empty());
    let user = user_owned.as_deref();
    let vid = vault_id(user);
    let gpg_id = gpg_id_for(user);
    let home = match ensure_gnupg_home(user) {
        Ok(h) => h,
        Err(e) => {
            return Json(CryptResponse { result: format!("ERROR_GNUPG_HOME:{}", e), signature: None });
        }
    };

    match req.mode.as_str() {
        "check" => {
            log_audit_event("gpg_check", "started", &format!("checking key for {} vid={}", gpg_id, vid));
            let exists = match gpg_list_secret(&gpg_id, &home).await {
                Ok(v) => v,
                Err(_) => {
                    log_audit_event("gpg_check", "failed", "GPG not found");
                    return Json(CryptResponse { result: "ERROR_GPG_NOT_FOUND".to_string(), signature: None });
                }
            };
            if !exists {
                return Json(CryptResponse { result: "UNINITIALIZED".to_string(), signature: None });
            }
            let result = if is_unsealed(&vid) {
                "UNSEALED".to_string()
            } else {
                "SEALED".to_string()
            };
            let signature = sign_response(&result);
            Json(CryptResponse { result, signature: Some(signature) })
        }

        "operator_unseal" => {
            if !is_convenience() {
                return Json(CryptResponse {
                    result: "ERROR_CUSTODY_STRICT".to_string(),
                    signature: None,
                });
            }
            match operator_unseal(&req.payload) {
                Ok(()) => {
                    log_audit_event("operator_unseal", "success", "KEK loaded");
                    let result = "OPERATOR_UNSEALED".to_string();
                    let signature = sign_response(&result);
                    Json(CryptResponse { result, signature: Some(signature) })
                }
                Err(e) => Json(CryptResponse { result: format!("ERROR_{}", e), signature: None }),
            }
        }

        "operator_status" => {
            let result = if !is_convenience() {
                "N_A_STRICT".to_string()
            } else if operator_sealed() {
                "OPERATOR_SEALED".to_string()
            } else {
                "OPERATOR_UNSEALED".to_string()
            };
            let signature = sign_response(&result);
            Json(CryptResponse { result, signature: Some(signature) })
        }

        "unlock" => {
            log_audit_event("vault_unlock", "attempted", &format!("vid={}", vid));
            let mut passphrase = req.payload.into_bytes();
            if let Err(e) = set_vault_key(&vid, passphrase.clone()) {
                passphrase.zeroize();
                return Json(CryptResponse { result: format!("ERROR_{}", e), signature: None });
            }
            // Convenience: also wrap for later OIDC-only unlock
            if is_convenience() && multiuser_enabled() {
                if let Some(sub) = user {
                    if !operator_sealed() {
                        let _ = wrap_and_store_passphrase(sub, &passphrase);
                    }
                }
            }
            passphrase.zeroize();
            log_audit_event("vault_unlock", "success", &format!("vid={}", vid));
            let result = "VAULT_UNSEALED".to_string();
            let signature = sign_response(&result);
            Json(CryptResponse { result, signature: Some(signature) })
        }

        "unlock_wrapped" => {
            // Convenience: unlock from wrapped blob after operator KEK is loaded.
            if !is_convenience() {
                return Json(CryptResponse { result: "ERROR_CUSTODY_STRICT".to_string(), signature: None });
            }
            let sub = match user {
                Some(s) => s,
                None => {
                    return Json(CryptResponse { result: "ERROR_USER_REQUIRED".to_string(), signature: None });
                }
            };
            if !has_wrapped_key(sub) {
                return Json(CryptResponse { result: "ERROR_NO_WRAPPED_KEY".to_string(), signature: None });
            }
            match unwrap_passphrase(sub) {
                Ok(mut pass) => {
                    let _ = set_vault_key(&vid, pass.clone());
                    pass.zeroize();
                    let result = "VAULT_UNSEALED".to_string();
                    let signature = sign_response(&result);
                    Json(CryptResponse { result, signature: Some(signature) })
                }
                Err(e) => Json(CryptResponse { result: format!("ERROR_{}", e), signature: None }),
            }
        }

        "seal" => {
            clear_vault_key(&vid);
            let result = "VAULT_SEALED".to_string();
            let signature = sign_response(&result);
            Json(CryptResponse { result, signature: Some(signature) })
        }

        "initialize" => {
            log_audit_event("gpg_init", "started", &format!("initializing key for {}", gpg_id));
            if gpg_list_secret(&gpg_id, &home).await.unwrap_or(false) {
                return Json(CryptResponse { result: "ERROR_ALREADY_INITIALIZED".to_string(), signature: None });
            }

            let passphrase = req.payload.clone();
            let key_type = req.key_type.clone().unwrap_or_else(|| "rsa4096".to_string());
            let (algo, length) = if key_type == "ed25519" {
                ("ed25519", "0")
            } else {
                ("rsa", "4096")
            };

            let gen_params = format!(
                "Key-Type: {}\nKey-Length: {}\nName-Real: Talos Vault\nName-Email: {}\nExpire-Date: 0\nPassphrase: {}\n%commit\n",
                algo, length, gpg_id, passphrase
            );

            let mut cmd = Command::new("gpg");
            apply_gnupg_env(&mut cmd, &home);
            let child = cmd
                .args(["--batch", "--gen-key"])
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
                    let trust_script = if let Some(ref h) = home {
                        format!(
                            "echo -e \"trust\\n5\\ny\\n\" | GNUPGHOME={} gpg --batch --command-fd 0 --edit-key {} >/dev/null 2>&1",
                            h.display(),
                            gpg_id
                        )
                    } else {
                        format!(
                            "echo -e \"trust\\n5\\ny\\n\" | gpg --batch --command-fd 0 --edit-key {} >/dev/null 2>&1",
                            gpg_id
                        )
                    };
                    let _ = Command::new("sh").args(["-c", &trust_script]).status().await;

                    let mut pass_bytes = passphrase.into_bytes();
                    let _ = set_vault_key(&vid, pass_bytes.clone());
                    if is_convenience() && multiuser_enabled() {
                        if let Some(sub) = user {
                            if !operator_sealed() {
                                let _ = wrap_and_store_passphrase(sub, &pass_bytes);
                            }
                        }
                    }
                    pass_bytes.zeroize();
                    Json(CryptResponse { result: "INITIALIZED".to_string(), signature: None })
                } else {
                    Json(CryptResponse { result: "ERROR_GEN".to_string(), signature: None })
                }
            } else {
                Json(CryptResponse { result: "ERROR_WAIT".to_string(), signature: None })
            }
        }

        "import" => {
            let key_data = req.payload;
            let passphrase = req.passphrase.clone().unwrap_or_default();

            let mut cmd = Command::new("gpg");
            apply_gnupg_env(&mut cmd, &home);
            let child = cmd
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
                    let mut pass_bytes = passphrase.into_bytes();
                    let _ = set_vault_key(&vid, pass_bytes.clone());
                    if is_convenience() && multiuser_enabled() {
                        if let Some(sub) = user {
                            if !operator_sealed() {
                                let _ = wrap_and_store_passphrase(sub, &pass_bytes);
                            }
                        }
                    }
                    pass_bytes.zeroize();
                    Json(CryptResponse { result: "INITIALIZED".to_string(), signature: None })
                } else {
                    Json(CryptResponse { result: "ERROR_IMPORT".to_string(), signature: None })
                }
            } else {
                Json(CryptResponse { result: "ERROR_WAIT".to_string(), signature: None })
            }
        }

        "export_key" => {
            let mut cmd = Command::new("gpg");
            apply_gnupg_env(&mut cmd, &home);
            let output = cmd
                .args(["--batch", "--export-secret-keys", "--armor", &gpg_id])
                .output()
                .await;
            match output {
                Ok(o) => Json(CryptResponse {
                    result: String::from_utf8_lossy(&o.stdout).to_string(),
                    signature: None,
                }),
                Err(_) => Json(CryptResponse { result: "ERROR_EXPORT".to_string(), signature: None }),
            }
        }

        "decrypt" | "encrypt" => {
            log_audit_event(&format!("gpg_{}", req.mode), "started", &format!("operation for {}", gpg_id));
            let passphrase = match get_vault_key(&vid) {
                Some(p) => p,
                None => {
                    return Json(CryptResponse { result: "ERROR_VAULT_SEALED".to_string(), signature: None });
                }
            };

            let input = req.payload;
            let decoded_input = match general_purpose::STANDARD.decode(&input) {
                Ok(decoded) => decoded,
                Err(_) => input.into_bytes(),
            };

            let passphrase_file = format!(
                "/tmp/gpg_passphrase_{}",
                if vid.is_empty() { "legacy".to_string() } else { vid.clone() }
            );
            if fs::write(&passphrase_file, &passphrase).is_err() {
                let mut p = passphrase;
                p.zeroize();
                return Json(CryptResponse { result: "ERROR_WRITE_PASSPHRASE_FILE".to_string(), signature: None });
            }
            let mut p = passphrase;
            p.zeroize();

            let mut final_args = vec![
                "--batch",
                "--pinentry-mode",
                "loopback",
                "--passphrase-file",
                passphrase_file.as_str(),
                "--trust-model",
                "always",
            ];
            if req.mode == "decrypt" {
                final_args.push("-d");
            } else {
                final_args.extend(["-e", "-r", gpg_id.as_str(), "--armor"]);
            }

            let mut cmd = Command::new("gpg");
            apply_gnupg_env(&mut cmd, &home);
            let child = cmd
                .args(&final_args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            let mut child = match child {
                Ok(c) => c,
                Err(_) => {
                    let _ = fs::remove_file(&passphrase_file);
                    return Json(CryptResponse { result: "ERROR_SPAWN".to_string(), signature: None });
                }
            };

            if let Some(mut stdin) = child.stdin.take() {
                if stdin.write_all(&decoded_input).await.is_err() {
                    let _ = fs::remove_file(&passphrase_file);
                    return Json(CryptResponse { result: "ERROR_WRITE_PAYLOAD".to_string(), signature: None });
                }
                drop(stdin);
            }

            let output = child.wait_with_output().await;
            let _ = fs::remove_file(&passphrase_file);

            match output {
                Ok(o) => {
                    let result = String::from_utf8_lossy(&o.stdout).to_string();
                    log_audit_event(&format!("gpg_{}", req.mode), "success", "operation completed");
                    let signature = sign_response(&result);
                    Json(CryptResponse { result, signature: Some(signature) })
                }
                Err(e) => {
                    log_audit_event(&format!("gpg_{}", req.mode), "failed", &format!("error: {}", e));
                    Json(CryptResponse { result: "ERROR_GPG_EXEC".to_string(), signature: None })
                }
            }
        }

        _ => Json(CryptResponse { result: "ERROR_INVALID_MODE".to_string(), signature: None }),
    }
}
