use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, StatusCode, header},
    Json,
    response::{IntoResponse, Response},
    middleware::Next,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use tower_sessions::Session;
use zeroize::Zeroize;
use crate::state::{AppState, RateLimiter, RateLimitEntry};
use crate::handlers::log_audit;

const MAX_LOGIN_ATTEMPTS: u32 = 5;
const RATE_LIMIT_WINDOW_SECONDS: u64 = 60;
const CSRF_TOKEN_KEY: &str = "csrf_token";

async fn generate_csrf_token(session: &Session) -> Result<String, StatusCode> {
    if let Some(token) = session.get::<String>(CSRF_TOKEN_KEY).await.unwrap_or(None) {
        return Ok(token);
    }
    
    // Generate simple random token
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let token = format!("csrf_{:x}", timestamp);
    
    session.insert(CSRF_TOKEN_KEY, &token).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(token)
}

pub async fn validate_csrf_token(session: &Session, token: &str) -> Result<bool, StatusCode> {
    let stored_token = session.get::<String>(CSRF_TOKEN_KEY).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    Ok(stored_token == token)
}

#[derive(Deserialize, Zeroize)]
#[zeroize(drop)]
pub struct LoginRequest {
    pub key: String,
}

fn check_rate_limit(ip: IpAddr, rate_limiter: &RateLimiter) -> bool {
    let mut limiter = rate_limiter.lock().unwrap();
    let now = Instant::now();
    
    if let Some(entry) = limiter.get_mut(&ip) {
        if now.duration_since(entry.window_start) > Duration::from_secs(RATE_LIMIT_WINDOW_SECONDS) {
            // Reset window
            entry.attempts = 1;
            entry.window_start = now;
            true
        } else {
            entry.attempts += 1;
            entry.attempts <= MAX_LOGIN_ATTEMPTS
        }
    } else {
        limiter.insert(ip, RateLimitEntry {
            attempts: 1,
            window_start: now,
        });
        true
    }
}

#[derive(Serialize)]
pub struct AuthStatus {
    pub initialized: bool,
    pub authenticated: bool,
    pub auth_method: Option<String>,
    pub bunker: bool,
}

pub async fn get_auth_status(
    State(_state): State<AppState>,
    session: Session,
) -> Json<AuthStatus> {
    let authenticated: bool = session.get("authenticated").await.unwrap_or_default().unwrap_or(false);
    let auth_method: Option<String> = session.get("auth_method").await.unwrap_or(None);

    // Check the actual system status through the health endpoint, which in turn queries the Bunker.
    let client = reqwest::Client::new();
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    let (initialized, bunker_ok) = match client.get(format!("{}/api/health", storage_url)).send().await {
        Ok(res) => {
            if let Ok(status) = res.json::<Value>().await {
                let bunker_status = status["bunker"].as_str().unwrap_or("OFFLINE");
                // The system is initialized if the Bunker is NOT "UNINITIALIZED".
                // It can be "SEALED" or "INITIALIZED", both count as initialized.
                let is_initialized = bunker_status != "UNINITIALIZED";
                let is_bunker_ok = bunker_status != "OFFLINE";
                (is_initialized, is_bunker_ok)
            } else {
                (false, false) // Assume worst case if the response is not valid JSON
            }
        },
        Err(_) => (false, false) // Assume worst case if Storage does not respond
    };
    
    Json(AuthStatus {
        initialized,
        authenticated,
        auth_method,
        bunker: bunker_ok,
    })
}

pub async fn login(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    // Check rate limiting
    if !check_rate_limit(addr.ip(), &state.rate_limiter) {
        log_audit(&state, &session, Some(addr.ip()), headers.get(header::USER_AGENT), "LOGIN_RATE_LIMITED", "system").await;
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error": "Too many login attempts. Please wait 60 seconds."})));
    }
    
    let client = reqwest::Client::new();
    let storage_url = std::env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());

    // Verify key with Storage (Unlock attempt & Verification)
    let res = client.post(format!("{}/api/unlock", storage_url))
        .json(&json!({ "key": payload.key }))
        .send().await;

    let ua_header = headers.get(header::USER_AGENT);

    match res {
        Ok(response) if response.status().is_success() => {
            session.insert("authenticated", true).await.unwrap();
            session.insert("auth_method", "password").await.unwrap();
            let csrf_token = generate_csrf_token(&session).await.unwrap_or_default();
            log_audit(&state, &session, Some(addr.ip()), ua_header, "LOGIN_SUCCESS", "system").await;
            (StatusCode::OK, Json(json!({
                "status": "Logged in",
                "csrf_token": csrf_token
            })))
        },
        _ => {
            log_audit(&state, &session, Some(addr.ip()), ua_header, "LOGIN_FAILURE", "system").await;
            (StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid Master Key"})))
        }
    }
}

pub async fn logout(session: Session) -> impl IntoResponse {
    session.flush().await.unwrap();
    (StatusCode::OK, Json(json!({"status": "Logged out"})))
}

pub async fn require_auth(session: Session, request: Request, next: Next) -> Result<Response, StatusCode> {
    let authenticated: bool = session.get("authenticated").await.unwrap_or_default().unwrap_or(false);
    if authenticated {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

pub async fn proxy_import_key(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Value>
) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    let client = reqwest::Client::new();
    
    let ua_header = headers.get(header::USER_AGENT);
    log_audit(&state, &session, Some(addr.ip()), ua_header, "IMPORT_SYSTEM", "system").await;

    let res = client.post(format!("{}/api/initialize/import", storage_url))
        .json(&body)
        .send().await;

    match res {
        Ok(response) => {
            let status = response.status();
            let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let body = response.json::<Value>().await.unwrap_or_default();
            (status_code, Json(body))
        },
        Err(_) => (StatusCode::BAD_GATEWAY, Json(json!({"error": "Storage unreachable"})))
    }
}

pub async fn proxy_backup_key(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    let ua_header = headers.get(header::USER_AGENT);
    log_audit(&state, &session, Some(addr.ip()), ua_header, "BACKUP_KEY", "system").await;

    // Proxy the download request
    let client = reqwest::Client::new();
    match client.get(format!("{}/api/backup/key", storage_url)).send().await {
        Ok(res) => {
            let bytes = res.bytes().await.unwrap_or_default();
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/pgp-keys"),
                    (header::CONTENT_DISPOSITION, "attachment; filename=\"talos_master_private.key\"")
                ],
                axum::body::Bytes::from(bytes)
            ).into_response()
        },
        Err(_) => (StatusCode::BAD_GATEWAY, Json(json!({"error": "Failed to retrieve key"}))).into_response()
    }
}