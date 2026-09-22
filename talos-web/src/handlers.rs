use axum::Json;
use axum::extract::{ConnectInfo, Multipart, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::io::{Cursor, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tower_sessions::Session;
use zip::write::FileOptions;
use crate::state::AppState;
use crate::auth::{validate_csrf_token, check_match_rate_limit, generate_csrf_token};
use crate::user_proxy::user_headers;

fn is_debug() -> bool {
    env::var("DEBUG").unwrap_or_default() == "true"
}

async fn session_user_sub(session: &Session) -> Option<String> {
    session.get::<String>("user_sub").await.ok().flatten()
}

fn bearer_from(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

async fn resolve_user_sub(
    state: &AppState,
    session: &Session,
    headers: &HeaderMap,
) -> Option<String> {
    if let Some(token) = bearer_from(headers) {
        if let Some(entry) = state.token_entry(token) {
            return entry.user_sub;
        }
    }
    session_user_sub(session).await
}

async fn proxy_request_with_user(
    url: &str,
    body: Option<Value>,
    user_sub: Option<&str>,
) -> (StatusCode, Json<Value>) {
    let client = reqwest::Client::new();
    let mut req = if let Some(b) = body {
        client.post(url).json(&b)
    } else {
        client.get(url)
    };
    for (k, v) in user_headers(user_sub) {
        req = req.header(k, v);
    }
    match req.send().await {
        Ok(res) => {
            let status = res.status();
            let data = match res.json::<Value>().await {
                Ok(d) => d,
                Err(_) => json!({"error": "Invalid node response"}),
            };
            if !status.is_success() {
                println!("⚠️ [WEB] Proxy Error [{}]: {:?}", status, data);
            }
            let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status_code, Json(data))
        }
        Err(e) => {
            println!("❌ [WEB] Node Unreachable: {}", e);
            (StatusCode::BAD_GATEWAY, Json(json!({"error": "Node unreachable"})))
        }
    }
}

async fn proxy_request_post_empty_user(url: &str, user_sub: Option<&str>) -> (StatusCode, Json<Value>) {
    let client = reqwest::Client::new();
    let mut req = client.post(url);
    for (k, v) in user_headers(user_sub) {
        req = req.header(k, v);
    }
    match req.send().await {
        Ok(res) => {
            let status = res.status();
            let data = match res.json::<Value>().await {
                Ok(d) => d,
                Err(_) => json!({"error": "Invalid node response"}),
            };
            let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status_code, Json(data))
        }
        Err(e) => {
            println!("❌ [WEB] Node Unreachable: {}", e);
            (StatusCode::BAD_GATEWAY, Json(json!({"error": "Node unreachable"})))
        }
    }
}

pub async fn get_version() -> Json<Value> {
    let version = env!("CARGO_PKG_VERSION");
    Json(json!({ "version": version }))
}

pub async fn health_check() -> Json<Value> {
    let client = reqwest::Client::new();
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    
    // Web layer only communicates with the Storage layer (Middleware)
    match client.get(format!("{}/api/health", storage_url)).send().await {
        Ok(res) => {
            let status = res.json::<Value>().await.unwrap_or(json!({
                "storage": true, 
                "bunker": false 
            }));
            Json(status)
        },
        Err(_) => Json(json!({ "storage": false, "bunker": false }))
    }
}

pub async fn proxy_list_tree(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying LIST TREE"); }
    let sub = resolve_user_sub(&state, &session, &headers).await;
    proxy_request_with_user(&format!("{}/api/tree", storage_url), None, sub.as_deref()).await
}

pub async fn proxy_match(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_match_rate_limit(addr.ip(), &state.match_rate_limiter) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error": "Too many match requests. Please wait."})),
        );
    }
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    let host = params.get("host").cloned().unwrap_or_default();
    if is_debug() { println!("--> [WEB] Proxying MATCH host={}", host); }

    let url = format!(
        "{}/api/match?host={}",
        storage_url,
        urlencoding_encode(&host)
    );
    let sub = resolve_user_sub(&state, &session, &headers).await;
    proxy_request_with_user(&url, None, sub.as_deref()).await
}

pub async fn proxy_reindex(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying MATCH REINDEX"); }
    let sub = resolve_user_sub(&state, &session, &headers).await;
    proxy_request_post_empty_user(&format!("{}/api/match/reindex", storage_url), sub.as_deref()).await
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}



pub async fn log_audit(
    state: &AppState,
    session: &Session,
    ip: Option<std::net::IpAddr>,
    user_agent: Option<&HeaderValue>,
    action: &str,
    target: &str,
) {
    let auth_method: Option<String> = session.get("auth_method").await.unwrap_or(None);
    let ip_str = ip.map(|i| i.to_string());
    let ua_str = user_agent.and_then(|ua| ua.to_str().ok());

    let _ = sqlx::query(
        "INSERT INTO audit_logs (action, target, ip_address, user_agent, auth_method) VALUES (?, ?, ?, ?, ?)",
    )
        .bind(action)
        .bind(target)
        .bind(ip_str)
        .bind(ua_str)
        .bind(auth_method)
        .execute(&state.pool)
        .await;
}

#[derive(Serialize, sqlx::FromRow)]
pub struct AuditLogEntry {
    id: i64,
    action: String,
    target: String,
    timestamp: String,
    ip_address: Option<String>,
    user_agent: Option<String>,
    auth_method: Option<String>,
}

pub async fn get_audit_logs(State(state): State<AppState>) -> Json<Vec<AuditLogEntry>> {
    let logs = sqlx::query_as::<_, AuditLogEntry>(
        "SELECT id, action, target, timestamp, ip_address, user_agent, auth_method FROM audit_logs ORDER BY id DESC LIMIT 100",
    )
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
    Json(logs)
}

pub async fn proxy_decrypt(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Value>
) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying DECRYPT"); }
    
    let path = body["path"].as_str().unwrap_or("unknown");
    let ua_header = headers.get(header::USER_AGENT);
    log_audit(&state, &session, Some(addr.ip()), ua_header, "DECRYPT", path).await;

    let sub = resolve_user_sub(&state, &session, &headers).await;
    proxy_request_with_user(&format!("{}/api/decrypt", storage_url), Some(body), sub.as_deref()).await
}

pub async fn proxy_save(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Value>
) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying SAVE"); }
    
    let path = body["path"].as_str().unwrap_or("unknown");
    let ua_header = headers.get(header::USER_AGENT);
    log_audit(&state, &session, Some(addr.ip()), ua_header, "SAVE", path).await;

    let sub = resolve_user_sub(&state, &session, &headers).await;
    proxy_request_with_user(&format!("{}/api/save", storage_url), Some(body), sub.as_deref()).await
}

pub async fn proxy_delete(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Value>
) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying DELETE"); }
    
    let path = body["path"].as_str().unwrap_or("unknown");
    let ua_header = headers.get(header::USER_AGENT);
    log_audit(&state, &session, Some(addr.ip()), ua_header, "DELETE", path).await;

    let sub = resolve_user_sub(&state, &session, &headers).await;
    proxy_request_with_user(&format!("{}/api/delete", storage_url), Some(body), sub.as_deref()).await
}

pub async fn proxy_create_category(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Value>
) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying CREATE CATEGORY"); }

    let path = body["path"].as_str().unwrap_or("unknown");
    let ua_header = headers.get(header::USER_AGENT);
    log_audit(&state, &session, Some(addr.ip()), ua_header, "CREATE_CATEGORY", path).await;

    let sub = resolve_user_sub(&state, &session, &headers).await;
    proxy_request_with_user(&format!("{}/api/create_category", storage_url), Some(body), sub.as_deref()).await
}

pub async fn proxy_initialize(
    State(state): State<AppState>,
    session: Session, // Empty session, but needed for signature
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Value>
) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying INITIALIZE"); }
    let ua_header = headers.get(header::USER_AGENT);
    log_audit(&state, &session, Some(addr.ip()), ua_header, "INITIALIZE", "system").await;
    let mut body = body;
    let sub = session_user_sub(&session).await;
    if state.oidc.enabled && sub.is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Sign in with Keycloak before initializing your vault"})),
        )
            .into_response();
    }
    if let (Some(obj), Some(s)) = (body.as_object_mut(), &sub) {
        obj.insert("user_sub".into(), json!(s));
    }
    let (status, Json(resp)) =
        proxy_request_with_user(&format!("{}/api/initialize", storage_url), Some(body), sub.as_deref()).await;
    if status.is_success() {
        // Initialize also unseals the bunker for this user — mark session ready.
        session.insert("vault_unlocked", true).await.ok();
        session.insert("authenticated", true).await.ok();
        if state.oidc.enabled {
            session.insert("auth_method", "oidc+vault").await.ok();
        } else {
            session.insert("auth_method", "password").await.ok();
        }
        let _ = generate_csrf_token(&session).await;
    }
    (status, Json(resp)).into_response()
}

pub async fn proxy_backup(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying BACKUP download"); }
    
    let client = reqwest::Client::new();

    let ua_header = headers.get(header::USER_AGENT);
    log_audit(&state, &session, Some(addr.ip()), ua_header, "BACKUP", "full_system").await;

    let sub = session_user_sub(&session).await;
    let mut req = client.get(format!("{}/api/backup", storage_url));
    for (k, v) in user_headers(sub.as_deref()) {
        req = req.header(k, v);
    }
    
    // 1. Obtener el backup de secretos (ZIP) del Storage
    match req.send().await {
        Ok(res) => {
            let secrets_zip_bytes = res.bytes().await.unwrap_or_default();
            
            // 2. Crear un nuevo ZIP maestro en memoria
            let mut buf = Vec::new();
            {
                let mut zip_writer = zip::ZipWriter::new(Cursor::new(&mut buf));
                let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);

                // A. Añadir secrets.zip
                let _ = zip_writer.start_file("secrets.zip", options);
                let _ = zip_writer.write_all(&secrets_zip_bytes);

                // The /data/talos.db path comes from docker-compose
                if let Ok(db_content) = std::fs::read("/data/talos.db") {
                    let _ = zip_writer.start_file("talos.db", options);
                    let _ = zip_writer.write_all(&db_content);
                } else {
                    println!("⚠️ [WEB] Could not read talos.db for backup");
                }
                
                let _ = zip_writer.finish();
            }

            (StatusCode::OK, [(header::CONTENT_TYPE, "application/zip"), (header::CONTENT_DISPOSITION, "attachment; filename=\"talos_full_backup.zip\"")], axum::body::Bytes::from(buf)).into_response()
        },
        Err(_) => {
            (StatusCode::BAD_GATEWAY, [(header::CONTENT_TYPE, "text/plain")], axum::body::Bytes::from("Error fetching backup")).into_response()
        }
    }
}

pub async fn proxy_restore(
    session: Session,
    mut multipart: Multipart
) -> impl IntoResponse {
    if is_debug() { println!("--> [WEB] RESTORE request received"); }

    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Processing RESTORE upload"); }
    
    // Validate CSRF token for state-changing operation
    let mut csrf_token = None;
    let mut backup_data = None;
    
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("csrf_token") {
            csrf_token = field.text().await.ok();
        } else if field.name() == Some("backup") {
            backup_data = Some(match field.bytes().await {
                Ok(b) => b,
                Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Failed to read backup data"}))),
            });
        }
    }
    
    if let Some(token) = csrf_token.as_ref() {
        if let Err(_) = validate_csrf_token(&session, token).await {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "CSRF token validation failed"})));
        }
    } else {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "CSRF token required"})));
    }
    
    let data = match backup_data {
        Some(d) => d,
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error": "No backup file provided"}))),
    };
    
    // 1. Intentar abrir el ZIP
    let reader = Cursor::new(&data);
    let mut archive = match zip::ZipArchive::new(reader) {
        Ok(a) => a,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid zip file"}))),
    };

    let mut secrets_payload: Option<Vec<u8>> = None;
    let mut db_restored = false;

    // 2. Find and restore talos.db (audit logs)
    if let Ok(mut db_file) = archive.by_name("talos.db") {
        let mut buf = Vec::new();
        if std::io::copy(&mut db_file, &mut buf).is_ok() {
            // Overwrite the local DB.
            // NOTE: In a high-concurrency environment this is risky,
            // but for a personal tool it is acceptable.
            if std::fs::write("/data/talos.db", buf).is_ok() {
                println!("--> [WEB] talos.db restored successfully");
                db_restored = true;
            }
        }
    }

    // 3. Find secrets.zip (The Storage backup)
    if let Ok(mut secrets_file) = archive.by_name("secrets.zip") {
        let mut buf = Vec::new();
        if std::io::copy(&mut secrets_file, &mut buf).is_ok() {
            secrets_payload = Some(buf);
        }
    }

    // 4. Determine what to send to Storage
    // If there's no secrets.zip or talos.db, we assume it's an old (Legacy) backup containing only secrets
    let payload_to_send = secrets_payload.unwrap_or_else(|| {
        if db_restored { Vec::new() } else { data.to_vec() }
    });

    if !payload_to_send.is_empty() {
        let client = reqwest::Client::new();
        let part = reqwest::multipart::Part::bytes(payload_to_send).file_name("backup.zip");
        let form = reqwest::multipart::Form::new().part("backup", part);

        if let Err(e) = client.post(format!("{}/api/restore", storage_url)).multipart(form).send().await {
             println!("❌ [WEB] Storage Restore Failed: {}", e);
             return (StatusCode::BAD_GATEWAY, Json(json!({"error": "Storage node unreachable"})));
        }
    }

    return (StatusCode::OK, Json(json!({"status": "System restored. Please refresh."})))
}
