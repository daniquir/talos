//! Authentication: legacy master-key login + OIDC (Keycloak) + vault unlock.

use axum::{
    extract::{ConnectInfo, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    Json,
    response::{IntoResponse, Redirect, Response},
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
use crate::settings::load_settings;
use crate::handlers::log_audit;
use crate::oidc::{
    custody_mode, exchange_code, pkce_challenge, random_string, validate_id_token,
};
use crate::user_proxy::{multiuser_enabled, user_headers};

const MAX_LOGIN_ATTEMPTS: u32 = 5;
const RATE_LIMIT_WINDOW_SECONDS: u64 = 60;
const MAX_MATCH_ATTEMPTS: u32 = 60;
const MATCH_RATE_WINDOW_SECONDS: u64 = 60;
const CSRF_TOKEN_KEY: &str = "csrf_token";

pub async fn generate_csrf_token(session: &Session) -> Result<String, StatusCode> {
    if let Some(token) = session.get::<String>(CSRF_TOKEN_KEY).await.unwrap_or(None) {
        return Ok(token);
    }
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
    check_rate_limit_n(ip, rate_limiter, MAX_LOGIN_ATTEMPTS, RATE_LIMIT_WINDOW_SECONDS)
}

pub fn check_rate_limit_n(
    ip: IpAddr,
    rate_limiter: &RateLimiter,
    max_attempts: u32,
    window_secs: u64,
) -> bool {
    let mut limiter = rate_limiter.lock().unwrap();
    let now = Instant::now();
    if let Some(entry) = limiter.get_mut(&ip) {
        if now.duration_since(entry.window_start) > Duration::from_secs(window_secs) {
            entry.attempts = 1;
            entry.window_start = now;
            true
        } else {
            entry.attempts += 1;
            entry.attempts <= max_attempts
        }
    } else {
        limiter.insert(ip, RateLimitEntry {
            attempts: 1,
            window_start: now,
        });
        true
    }
}

pub fn check_match_rate_limit(ip: IpAddr, rate_limiter: &RateLimiter) -> bool {
    check_rate_limit_n(ip, rate_limiter, MAX_MATCH_ATTEMPTS, MATCH_RATE_WINDOW_SECONDS)
}

#[derive(Serialize)]
pub struct AuthStatus {
    pub initialized: bool,
    pub authenticated: bool,
    pub auth_method: Option<String>,
    pub bunker: bool,
    pub lang: String,
    pub multiuser: bool,
    pub custody_mode: String,
    pub oidc_enabled: bool,
    pub oidc_authenticated: bool,
    pub vault_unlocked: bool,
    pub user_sub: Option<String>,
    pub is_admin: bool,
}

pub async fn get_auth_status(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
) -> Json<AuthStatus> {
    let mut authenticated = false;
    let mut auth_method: Option<String> = session.get("auth_method").await.unwrap_or(None);
    let oidc_authenticated: bool = session.get("oidc_authenticated").await.unwrap_or_default().unwrap_or(false);
    let mut vault_unlocked: bool = session.get("vault_unlocked").await.unwrap_or_default().unwrap_or(false);
    let mut user_sub: Option<String> = session.get("user_sub").await.unwrap_or(None);
    let is_admin: bool = session.get("is_admin").await.unwrap_or_default().unwrap_or(false);

    if let Some(token) = bearer_token(&headers) {
        if let Some(entry) = state.token_entry(token) {
            auth_method = Some("token".to_string());
            vault_unlocked = entry.vault_unlocked;
            user_sub = entry.user_sub.clone();
            authenticated = entry.vault_unlocked;
        }
    } else if state.oidc.enabled {
        // Resume vault access if OIDC session is alive and bunker still holds the key.
        if oidc_authenticated && !vault_unlocked {
            if bunker_already_unsealed(user_sub.as_deref()).await {
                session.insert("vault_unlocked", true).await.ok();
                session.insert("authenticated", true).await.ok();
                vault_unlocked = true;
                if auth_method.is_none() {
                    auth_method = Some("oidc+vault".to_string());
                    session.insert("auth_method", "oidc+vault").await.ok();
                }
            }
        }
        authenticated = oidc_authenticated && vault_unlocked;
    } else {
        authenticated = session.get("authenticated").await.unwrap_or_default().unwrap_or(false);
    }

    let client = reqwest::Client::new();
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());

    // When OIDC identity is present, check *that user's* vault — not the legacy shared keyring.
    let mut health_req = client.get(format!("{}/api/health", storage_url));
    for (k, v) in user_headers(user_sub.as_deref()) {
        health_req = health_req.header(k, v);
    }

    let (initialized, bunker_ok) = match health_req.send().await {
        Ok(res) => {
            if let Ok(status) = res.json::<Value>().await {
                let bunker_status = status["bunker"].as_str().unwrap_or("OFFLINE");
                // Permission / home errors count as not ready; treat as uninitialized for setup UI.
                let is_initialized = bunker_status != "UNINITIALIZED"
                    && !bunker_status.starts_with("ERROR_GNUPG_HOME")
                    && bunker_status != "ERROR_GPG_NOT_FOUND";
                let is_bunker_ok = bunker_status != "OFFLINE" && !bunker_status.starts_with("ERROR_");
                (is_initialized, is_bunker_ok || bunker_status == "UNINITIALIZED")
            } else {
                (false, false)
            }
        }
        Err(_) => (false, false),
    };

    let settings = load_settings(&state.pool).await;

    Json(AuthStatus {
        initialized,
        authenticated,
        auth_method,
        bunker: bunker_ok,
        lang: settings.lang,
        multiuser: multiuser_enabled(),
        custody_mode: custody_mode(),
        oidc_enabled: state.oidc.enabled,
        oidc_authenticated,
        vault_unlocked,
        user_sub,
        is_admin,
    })
}

async fn unlock_with_key(key: &str, user_sub: Option<&str>) -> Result<(), StatusCode> {
    let client = reqwest::Client::new();
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    let mut req = client
        .post(format!("{}/api/unlock", storage_url))
        .json(&json!({ "key": key, "user_sub": user_sub }));
    for (k, v) in user_headers(user_sub) {
        req = req.header(k, v);
    }
    let res = req.send().await;
    match res {
        Ok(response) if response.status().is_success() => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

async fn initialize_with_key(key: &str, user_sub: Option<&str>) -> Result<(), StatusCode> {
    let client = reqwest::Client::new();
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    let mut req = client
        .post(format!("{}/api/initialize", storage_url))
        .json(&json!({ "key": key, "user_sub": user_sub }));
    for (k, v) in user_headers(user_sub) {
        req = req.header(k, v);
    }
    match req.send().await {
        Ok(response) if response.status().is_success() => Ok(()),
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

async fn vault_uninitialized(user_sub: Option<&str>) -> bool {
    let client = reqwest::Client::new();
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    let mut req = client.get(format!("{}/api/health", storage_url));
    for (k, v) in user_headers(user_sub) {
        req = req.header(k, v);
    }
    match req.send().await {
        Ok(res) => res
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v["bunker"].as_str().map(|s| s == "UNINITIALIZED" || s.starts_with("ERROR_GNUPG_HOME")))
            .unwrap_or(false),
        Err(_) => false,
    }
}

async fn unlock_wrapped(user_sub: &str) -> Result<(), StatusCode> {
    let client = reqwest::Client::new();
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    let mut req = client.post(format!("{}/api/unlock/wrapped", storage_url));
    for (k, v) in user_headers(Some(user_sub)) {
        req = req.header(k, v);
    }
    match req.send().await {
        Ok(response) if response.status().is_success() => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

async fn bunker_already_unsealed(user_sub: Option<&str>) -> bool {
    let client = reqwest::Client::new();
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    let mut req = client.get(format!("{}/api/health", storage_url));
    for (k, v) in user_headers(user_sub) {
        req = req.header(k, v);
    }
    match req.send().await {
        Ok(res) => res
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v["bunker"].as_str().map(|s| s == "UNSEALED"))
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Legacy single-tenant login (MULTIUSER=false) or vault unlock when already OIDC'd.
pub async fn login(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    if !check_rate_limit(addr.ip(), &state.rate_limiter) {
        log_audit(&state, &session, Some(addr.ip()), headers.get(header::USER_AGENT), "LOGIN_RATE_LIMITED", "system").await;
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error": "Too many login attempts. Please wait 60 seconds."})));
    }

    let ua_header = headers.get(header::USER_AGENT);
    let user_sub: Option<String> = session.get("user_sub").await.unwrap_or(None);

    if state.oidc.enabled {
        let oidc_ok: bool = session.get("oidc_authenticated").await.unwrap_or_default().unwrap_or(false);
        if !oidc_ok {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "Sign in with Keycloak first"})));
        }
    }

    match unlock_with_key(&payload.key, user_sub.as_deref()).await {
        Ok(()) => {
            session.insert("authenticated", true).await.unwrap();
            session.insert("vault_unlocked", true).await.unwrap();
            if !state.oidc.enabled {
                session.insert("auth_method", "password").await.unwrap();
            } else {
                session.insert("auth_method", "oidc+vault").await.unwrap();
            }
            let csrf_token = generate_csrf_token(&session).await.unwrap_or_default();
            log_audit(&state, &session, Some(addr.ip()), ua_header, "LOGIN_SUCCESS", user_sub.as_deref().unwrap_or("system")).await;
            (StatusCode::OK, Json(json!({
                "status": "Logged in",
                "csrf_token": csrf_token
            })))
        }
        Err(_) => {
            log_audit(&state, &session, Some(addr.ip()), ua_header, "LOGIN_FAILURE", "system").await;
            (StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid Master Key"})))
        }
    }
}

pub async fn issue_token(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    if !check_rate_limit(addr.ip(), &state.rate_limiter) {
        log_audit(&state, &session, Some(addr.ip()), headers.get(header::USER_AGENT), "TOKEN_RATE_LIMITED", "system").await;
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error": "Too many login attempts. Please wait 60 seconds."})));
    }

    let ua_header = headers.get(header::USER_AGENT);
    let user_sub: Option<String> = session.get("user_sub").await.unwrap_or(None);

    if state.oidc.enabled {
        let oidc_ok: bool = session.get("oidc_authenticated").await.unwrap_or_default().unwrap_or(false);
        if !oidc_ok && user_sub.is_none() {
            // Extension may send OIDC access token later; for now require prior oidc session or body
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "OIDC required"})));
        }
    }

    match unlock_with_key(&payload.key, user_sub.as_deref()).await {
        Ok(()) => {
            let access_token = state.issue_token(user_sub.clone(), true);
            log_audit(&state, &session, Some(addr.ip()), ua_header, "TOKEN_ISSUED", user_sub.as_deref().unwrap_or("extension")).await;
            (StatusCode::OK, Json(json!({
                "access_token": access_token,
                "token_type": "Bearer",
                "expires_in": crate::state::API_TOKEN_TTL.as_secs()
            })))
        }
        Err(_) => {
            log_audit(&state, &session, Some(addr.ip()), ua_header, "TOKEN_FAILURE", "extension").await;
            (StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid Master Key"})))
        }
    }
}

#[derive(Deserialize)]
pub struct ExtTokenRequest {
    pub key: Option<String>,
    pub id_token: Option<String>,
}

/// Extension: exchange OIDC id_token (+ optional vault key in strict mode) for Bearer.
pub async fn issue_token_oidc(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<ExtTokenRequest>,
) -> impl IntoResponse {
    if !state.oidc.enabled {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "OIDC disabled"})));
    }
    let id_token = match payload.id_token.as_deref() {
        Some(t) => t,
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error": "id_token required"}))),
    };
    let claims = match validate_id_token(&state.oidc, &state.jwks, id_token).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::UNAUTHORIZED, Json(json!({"error": e}))),
    };

    let sub = claims.sub.clone();
    let mut unlocked = false;

    if custody_mode() == "convenience" {
        if unlock_wrapped(&sub).await.is_ok() {
            unlocked = true;
        } else if let Some(ref key) = payload.key {
            if unlock_with_key(key, Some(&sub)).await.is_ok() {
                unlocked = true;
            } else if vault_uninitialized(Some(&sub)).await
                && initialize_with_key(key, Some(&sub)).await.is_ok()
            {
                unlocked = true;
            }
        }
    } else {
        let key = match payload.key.as_deref() {
            Some(k) if !k.is_empty() => k,
            _ => return (StatusCode::UNAUTHORIZED, Json(json!({"error": "Vault passphrase required"}))),
        };
        if unlock_with_key(key, Some(&sub)).await.is_ok() {
            unlocked = true;
        } else if vault_uninitialized(Some(&sub)).await
            && initialize_with_key(key, Some(&sub)).await.is_ok()
        {
            unlocked = true;
        }
    }

    if !unlocked {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "Vault unlock failed"})));
    }

    let access_token = state.issue_token(Some(sub.clone()), true);
    log_audit(&state, &session, Some(addr.ip()), headers.get(header::USER_AGENT), "TOKEN_ISSUED_OIDC", &sub).await;
    (StatusCode::OK, Json(json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": crate::state::API_TOKEN_TTL.as_secs(),
        "user_sub": sub
    })))
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

pub async fn logout(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(token) = bearer_token(&headers) {
        state.revoke_token(token);
    }
    session.flush().await.unwrap();
    (StatusCode::OK, Json(json!({"status": "Logged out"})))
}

pub async fn oidc_login(State(state): State<AppState>, session: Session) -> impl IntoResponse {
    if !state.oidc.enabled {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "OIDC not configured"}))).into_response();
    }
    let verifier = random_string(32);
    let challenge = pkce_challenge(&verifier);
    let state_csrf = random_string(16);
    session.insert("oidc_verifier", verifier).await.ok();
    session.insert("oidc_state", state_csrf.clone()).await.ok();
    let url = state.oidc.authorize_url(&state_csrf, &challenge);
    Redirect::temporary(&url).into_response()
}

#[derive(Deserialize)]
pub struct OidcCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

pub async fn oidc_callback(
    State(state): State<AppState>,
    session: Session,
    Query(q): Query<OidcCallbackQuery>,
) -> impl IntoResponse {
    if let Some(err) = q.error {
        return Redirect::temporary(&format!("/?oidc_error={}", urlencoding::encode(&err))).into_response();
    }
    let code = match q.code {
        Some(c) => c,
        None => return Redirect::temporary("/?oidc_error=missing_code").into_response(),
    };
    let expected_state: Option<String> = session.get("oidc_state").await.unwrap_or(None);
    if expected_state.as_deref() != q.state.as_deref() {
        return Redirect::temporary("/?oidc_error=state_mismatch").into_response();
    }
    let verifier: String = match session.get("oidc_verifier").await.unwrap_or(None) {
        Some(v) => v,
        None => return Redirect::temporary("/?oidc_error=missing_verifier").into_response(),
    };

    let (id_token, _) = match exchange_code(&state.oidc, &code, &verifier).await {
        Ok(v) => v,
        Err(e) => return Redirect::temporary(&format!("/?oidc_error={}", urlencoding::encode(&e))).into_response(),
    };
    let claims = match validate_id_token(&state.oidc, &state.jwks, &id_token).await {
        Ok(c) => c,
        Err(e) => return Redirect::temporary(&format!("/?oidc_error={}", urlencoding::encode(&e))).into_response(),
    };

    session.insert("oidc_authenticated", true).await.ok();
    session.insert("user_sub", claims.sub.clone()).await.ok();
    session.insert("is_admin", claims.is_admin()).await.ok();
    session.insert("auth_method", "oidc").await.ok();
    if let Some(email) = claims.email.clone() {
        session.insert("email", email).await.ok();
    }
    let _ = generate_csrf_token(&session).await;

    let mut vault_unlocked = false;
    if custody_mode() == "convenience" {
        if unlock_wrapped(&claims.sub).await.is_ok() {
            vault_unlocked = true;
            session.insert("vault_unlocked", true).await.ok();
            session.insert("authenticated", true).await.ok();
        }
    }
    // If the bunker still holds this user's key in RAM (e.g. web session timed out
    // but bunker did not restart), skip asking for the vault passphrase again.
    if !vault_unlocked && bunker_already_unsealed(Some(&claims.sub)).await {
        vault_unlocked = true;
        session.insert("vault_unlocked", true).await.ok();
        session.insert("authenticated", true).await.ok();
        if custody_mode() == "convenience" {
            session.insert("auth_method", "oidc").await.ok();
        } else {
            session.insert("auth_method", "oidc+vault").await.ok();
        }
    }

    if vault_unlocked {
        return Redirect::temporary("/").into_response();
    }
    Redirect::temporary("/?need_vault_unlock=1").into_response()
}

#[derive(Deserialize, Zeroize)]
#[zeroize(drop)]
pub struct OperatorUnsealBody {
    pub key: String,
}

pub async fn operator_unseal(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<OperatorUnsealBody>,
) -> impl IntoResponse {
    let is_admin: bool = session.get("is_admin").await.unwrap_or_default().unwrap_or(false);
    if !is_admin {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "talos-admin role required"})));
    }
    let client = reqwest::Client::new();
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    match client
        .post(format!("{}/api/operator/unseal", storage_url))
        .json(&json!({ "key": body.key }))
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            (StatusCode::OK, Json(json!({"status": "operator_unsealed"})))
        }
        _ => (StatusCode::BAD_REQUEST, Json(json!({"error": "Operator unseal failed"}))),
    }
}

pub async fn require_auth(
    State(state): State<AppState>,
    session: Session,
    request: Request,
    next: Next,
) -> Response {
    let unauthorized = || {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Unauthorized — session expired or vault locked"})),
        )
            .into_response()
    };

    if let Some(token) = bearer_token(request.headers()) {
        if let Some(entry) = state.token_entry(token) {
            if entry.vault_unlocked {
                return next.run(request).await;
            }
            return unauthorized();
        }
        return unauthorized();
    }

    if state.oidc.enabled {
        let oidc_ok: bool = session.get("oidc_authenticated").await.unwrap_or_default().unwrap_or(false);
        let vault_ok: bool = session.get("vault_unlocked").await.unwrap_or_default().unwrap_or(false);
        if oidc_ok && vault_ok {
            return next.run(request).await;
        }
        return unauthorized();
    }

    let authenticated: bool = session.get("authenticated").await.unwrap_or_default().unwrap_or(false);
    if authenticated {
        next.run(request).await
    } else {
        unauthorized()
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

    let user_sub: Option<String> = session.get("user_sub").await.unwrap_or(None);
    let mut body = body;
    if let Some(obj) = body.as_object_mut() {
        if let Some(sub) = &user_sub {
            obj.insert("user_sub".into(), json!(sub));
        }
    }
    let mut req = client.post(format!("{}/api/initialize/import", storage_url)).json(&body);
    for (k, v) in user_headers(user_sub.as_deref()) {
        req = req.header(k, v);
    }
    let res = req.send().await;

    match res {
        Ok(response) => {
            let status = response.status();
            let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let body = response.json::<Value>().await.unwrap_or_default();
            (status_code, Json(body))
        }
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

    let user_sub: Option<String> = session.get("user_sub").await.unwrap_or(None);
    let client = reqwest::Client::new();
    let mut req = client.get(format!("{}/api/backup/key", storage_url));
    for (k, v) in user_headers(user_sub.as_deref()) {
        req = req.header(k, v);
    }
    match req.send().await {
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
        }
        Err(_) => (StatusCode::BAD_GATEWAY, Json(json!({"error": "Failed to retrieve key"}))).into_response()
    }
}
