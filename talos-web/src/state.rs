use sqlx::SqlitePool;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use crate::oidc::{JwksCache, OidcConfig};

pub struct RateLimitEntry {
    pub attempts: u32,
    pub window_start: Instant,
}

pub type RateLimiter = Arc<Mutex<HashMap<IpAddr, RateLimitEntry>>>;

pub struct ApiTokenEntry {
    pub expires_at: Instant,
    pub user_sub: Option<String>,
    pub vault_unlocked: bool,
}

/// In-memory Bearer tokens for extension / API clients (not cookie sessions).
pub type TokenStore = Arc<Mutex<HashMap<String, ApiTokenEntry>>>;

/// Extension / API Bearer lifetime (short window; unlock again after expiry).
pub const API_TOKEN_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub rate_limiter: RateLimiter,
    pub match_rate_limiter: RateLimiter,
    pub token_store: TokenStore,
    pub oidc: OidcConfig,
    pub jwks: JwksCache,
}

impl AppState {
    pub fn issue_token(&self, user_sub: Option<String>, vault_unlocked: bool) -> String {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = hex::encode(bytes);
        let mut store = self.token_store.lock().unwrap();
        store.insert(
            token.clone(),
            ApiTokenEntry {
                expires_at: Instant::now() + API_TOKEN_TTL,
                user_sub,
                vault_unlocked,
            },
        );
        token
    }

    pub fn token_entry(&self, token: &str) -> Option<ApiTokenEntry> {
        let mut store = self.token_store.lock().unwrap();
        let now = Instant::now();
        store.retain(|_, entry| entry.expires_at > now);
        store.get(token).cloned()
    }

    pub fn token_valid(&self, token: &str) -> bool {
        self.token_entry(token).is_some()
    }

    pub fn revoke_token(&self, token: &str) {
        let mut store = self.token_store.lock().unwrap();
        store.remove(token);
    }
}

impl Clone for ApiTokenEntry {
    fn clone(&self) -> Self {
        Self {
            expires_at: self.expires_at,
            user_sub: self.user_sub.clone(),
            vault_unlocked: self.vault_unlocked,
        }
    }
}
