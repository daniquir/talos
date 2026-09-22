//! Forward identity to storage with HMAC-signed headers.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use hex;
use std::env;

type HmacSha256 = Hmac<Sha256>;

pub const HDR_USER_SUB: &str = "X-Talos-User-Sub";
pub const HDR_USER_SIG: &str = "X-Talos-User-Sig";

pub fn multiuser_enabled() -> bool {
    matches!(
        env::var("MULTIUSER")
            .unwrap_or_else(|_| "true".to_string())
            .to_lowercase()
            .as_str(),
        "1" | "true" | "yes"
    )
}

pub fn sign_sub(sub: &str) -> String {
    let secret = env::var("SHARED_SECRET").unwrap_or_default();
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(sub.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn user_headers(sub: Option<&str>) -> Vec<(String, String)> {
    if !multiuser_enabled() {
        return vec![];
    }
    match sub {
        Some(s) if !s.is_empty() => vec![
            (HDR_USER_SUB.to_string(), s.to_string()),
            (HDR_USER_SIG.to_string(), sign_sub(s)),
        ],
        _ => vec![],
    }
}
