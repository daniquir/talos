use sqlx::SqlitePool;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct RateLimitEntry {
    pub attempts: u32,
    pub window_start: Instant,
}

pub type RateLimiter = Arc<Mutex<HashMap<IpAddr, RateLimitEntry>>>;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub rate_limiter: RateLimiter,
}