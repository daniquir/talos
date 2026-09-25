//! Resolve per-user password-store roots and verify web→storage identity headers.

use axum::http::{HeaderMap, StatusCode};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use hex;
use std::env;
use std::fs;
use std::path::Path;
use crate::config::STORE_PATH;

type HmacSha256 = Hmac<Sha256>;

pub const HDR_USER_SUB: &str = "x-talos-user-sub";
pub const HDR_USER_SIG: &str = "x-talos-user-sig";

pub fn multiuser_enabled() -> bool {
    matches!(
        env::var("MULTIUSER")
            .unwrap_or_else(|_| "true".to_string())
            .to_lowercase()
            .as_str(),
        "1" | "true" | "yes"
    )
}

pub fn sanitize_sub(sub: &str) -> Result<String, StatusCode> {
    let s = sub.trim();
    if s.is_empty() || s.len() > 128 {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':' || c == '.')
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(s.replace(':', "_"))
}

fn verify_sig(sub: &str, sig_hex: &str) -> bool {
    let secret = env::var("SHARED_SECRET").unwrap_or_default();
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(sub.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    ct_eq(&expected, sig_hex)
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

/// Extract verified Keycloak `sub` (or None in legacy single-tenant mode).
///
/// If multiuser is on and identity headers are missing, returns `Ok(None)` so
/// routes like health can stay anonymous. Routes that require a user should
/// reject `None` themselves.
pub fn extract_user_sub(headers: &HeaderMap) -> Result<Option<String>, StatusCode> {
    if !multiuser_enabled() {
        return Ok(None);
    }
    let sub = match headers
        .get(HDR_USER_SUB)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
    {
        Some(s) => s,
        None => return Ok(None),
    };
    let sig = headers
        .get(HDR_USER_SIG)
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !verify_sig(&sub, sig) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let _ = sanitize_sub(&sub)?;
    Ok(Some(sub))
}

/// Absolute password-store root for this request.
pub fn user_store_root(user_sub: Option<&str>) -> String {
    let base = STORE_PATH.as_str();
    match user_sub {
        Some(sub) if multiuser_enabled() => {
            let id = sanitize_sub(sub).unwrap_or_else(|_| "invalid".into());
            format!("{}/users/{}", base, id)
        }
        _ => base.to_string(),
    }
}

pub fn ensure_user_store(user_sub: Option<&str>) -> Result<String, StatusCode> {
    let root = user_store_root(user_sub);
    fs::create_dir_all(&root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Ensure .gpg-id exists for pass-style tooling
    let gpg_id_file = format!("{}/.gpg-id", root);
    if !Path::new(&gpg_id_file).exists() {
        let gpg_id = match user_sub {
            Some(sub) if multiuser_enabled() => {
                let id = sanitize_sub(sub).unwrap_or_else(|_| "user".into());
                format!("{}@talos.local", id)
            }
            _ => env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string()),
        };
        let _ = fs::write(gpg_id_file, gpg_id);
    }
    Ok(root)
}

/// When MULTIUSER is on, vault routes require a verified identity header.
pub fn require_user_sub(headers: &HeaderMap) -> Result<Option<String>, StatusCode> {
    let user = extract_user_sub(headers)?;
    if multiuser_enabled() && user.is_none() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(user)
}

/// Sign a sub the same way web does (for tests / internal use).
pub fn sign_sub(sub: &str) -> String {
    let secret = env::var("SHARED_SECRET").unwrap_or_default();
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac");
    mac.update(sub.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}
