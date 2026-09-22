//! Per-user GPG home + passphrase map + optional KEK wrapping (convenience custody).

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use zeroize::Zeroize;

/// In-memory passphrases keyed by vault id (`""` = legacy single-tenant).
pub static VAULT_KEYS: Lazy<Mutex<HashMap<String, Vec<u8>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Deployment KEK for convenience mode (operator-unsealed).
pub static OPERATOR_KEK: Lazy<Mutex<Option<[u8; 32]>>> = Lazy::new(|| Mutex::new(None));

pub fn multiuser_enabled() -> bool {
    matches!(
        env::var("MULTIUSER").unwrap_or_else(|_| "true".to_string()).to_lowercase().as_str(),
        "1" | "true" | "yes"
    )
}

pub fn custody_mode() -> String {
    env::var("TALOS_CUSTODY_MODE")
        .unwrap_or_else(|_| "strict".to_string())
        .to_lowercase()
}

pub fn is_convenience() -> bool {
    custody_mode() == "convenience"
}

/// Sanitize Keycloak `sub` for filesystem paths.
pub fn sanitize_sub(sub: &str) -> Result<String, String> {
    let s = sub.trim();
    if s.is_empty() || s.len() > 128 {
        return Err("invalid user_sub".into());
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':' || c == '.')
    {
        return Err("invalid user_sub characters".into());
    }
    // Filesystem-safe: replace : with _
    Ok(s.replace(':', "_"))
}

pub fn vault_id(user_sub: Option<&str>) -> String {
    if !multiuser_enabled() {
        return String::new();
    }
    match user_sub {
        Some(s) if !s.is_empty() => sanitize_sub(s).unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn gpg_id_for(user_sub: Option<&str>) -> String {
    if !multiuser_enabled() || user_sub.map(|s| s.is_empty()).unwrap_or(true) {
        return env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string());
    }
    let id = vault_id(user_sub);
    format!("{}@talos.local", id)
}

pub fn gnupg_home_for(user_sub: Option<&str>) -> Option<PathBuf> {
    if !multiuser_enabled() || user_sub.map(|s| s.is_empty()).unwrap_or(true) {
        return None; // use default ~/.gnupg
    }
    let base = env::var("GNUPG_USERS_DIR").unwrap_or_else(|_| "/home/talos/.gnupg-users".into());
    let id = vault_id(user_sub);
    if id.is_empty() {
        return None;
    }
    Some(PathBuf::from(base).join(id))
}

pub fn ensure_gnupg_home(user_sub: Option<&str>) -> Result<Option<PathBuf>, String> {
    let home = gnupg_home_for(user_sub);
    if let Some(ref p) = home {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
        // Restrictive perms best-effort
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o700));
        }
    }
    Ok(home)
}

fn wrapped_path(user_sub: &str) -> PathBuf {
    let base = env::var("WRAPPED_KEYS_DIR").unwrap_or_else(|_| "/home/talos/.talos-wrapped".into());
    let id = sanitize_sub(user_sub).unwrap_or_else(|_| "invalid".into());
    PathBuf::from(base).join(format!("{}.wrap", id))
}

fn derive_kek(raw: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"talos-operator-kek-v1:");
    hasher.update(raw.as_bytes());
    let out = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&out);
    key
}

pub fn operator_unseal(kek_passphrase: &str) -> Result<(), String> {
    let key = derive_kek(kek_passphrase);
    let mut guard = OPERATOR_KEK.lock().map_err(|_| "lock failed")?;
    *guard = Some(key);
    Ok(())
}

pub fn operator_sealed() -> bool {
    OPERATOR_KEK
        .lock()
        .map(|g| g.is_none())
        .unwrap_or(true)
}

pub fn wrap_and_store_passphrase(user_sub: &str, passphrase: &[u8]) -> Result<(), String> {
    let kek = {
        let guard = OPERATOR_KEK.lock().map_err(|_| "lock failed")?;
        *guard.as_ref().ok_or("operator KEK sealed")?
    };
    let cipher = Aes256Gcm::new_from_slice(&kek).map_err(|e| e.to_string())?;
    let mut nonce_bytes = [0u8; 12];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, passphrase)
        .map_err(|e| e.to_string())?;
    let mut blob = Vec::with_capacity(12 + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    let path = wrapped_path(user_sub);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&path, blob).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn unwrap_passphrase(user_sub: &str) -> Result<Vec<u8>, String> {
    let kek = {
        let guard = OPERATOR_KEK.lock().map_err(|_| "lock failed")?;
        *guard.as_ref().ok_or("operator KEK sealed")?
    };
    let path = wrapped_path(user_sub);
    let blob = fs::read(&path).map_err(|_| "no wrapped key for user".to_string())?;
    if blob.len() < 13 {
        return Err("corrupt wrapped key".into());
    }
    let (nonce_bytes, ct) = blob.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(&kek).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ct)
        .map_err(|_| "unwrap failed".to_string())
}

pub fn has_wrapped_key(user_sub: &str) -> bool {
    wrapped_path(user_sub).exists()
}

pub fn set_vault_key(vid: &str, passphrase: Vec<u8>) -> Result<(), String> {
    let mut guard = VAULT_KEYS.lock().map_err(|_| "lock failed")?;
    guard.insert(vid.to_string(), passphrase);
    Ok(())
}

pub fn clear_vault_key(vid: &str) {
    if let Ok(mut guard) = VAULT_KEYS.lock() {
        if let Some(mut v) = guard.remove(vid) {
            v.zeroize();
        }
    }
}

pub fn get_vault_key(vid: &str) -> Option<Vec<u8>> {
    VAULT_KEYS
        .lock()
        .ok()
        .and_then(|g| g.get(vid).cloned())
}

pub fn is_unsealed(vid: &str) -> bool {
    VAULT_KEYS
        .lock()
        .map(|g| g.contains_key(vid))
        .unwrap_or(false)
}
