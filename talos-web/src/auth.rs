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
use std::net::SocketAddr;
use tower_sessions::Session;
use zeroize::Zeroize;
use crate::state::AppState;
use crate::handlers::log_audit;

#[derive(Deserialize, Zeroize)]
#[zeroize(drop)]
pub struct LoginRequest {
    pub key: String,
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
            log_audit(&state, &session, Some(addr.ip()), ua_header, "LOGIN_SUCCESS", "system").await;
            (StatusCode::OK, Json(json!({"status": "Logged in"})))
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