use axum::Json;
use axum::extract::{ConnectInfo, Multipart, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use serde::Serialize;
use serde_json::{json, Value};
use std::env;
use std::io::{Cursor, Write};
use std::net::SocketAddr;
use tower_sessions::Session;
use zip::write::FileOptions;
use crate::state::AppState;

fn is_debug() -> bool {
    env::var("DEBUG").unwrap_or_default() == "true"
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

pub async fn proxy_list_tree() -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying LIST TREE"); }
    proxy_request(&format!("{}/api/tree", storage_url), None).await
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

    proxy_request(&format!("{}/api/decrypt", storage_url), Some(body)).await
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

    proxy_request(&format!("{}/api/save", storage_url), Some(body)).await
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

    proxy_request(&format!("{}/api/delete", storage_url), Some(body)).await
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

    proxy_request(&format!("{}/api/create_category", storage_url), Some(body)).await
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
    proxy_request(&format!("{}/api/initialize", storage_url), Some(body)).await
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
    
    // 1. Obtener el backup de secretos (ZIP) del Storage
    match client.get(format!("{}/api/backup", storage_url)).send().await {
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
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    mut multipart: Multipart
) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Processing RESTORE upload"); }
    
    let ua_header = headers.get(header::USER_AGENT);
    log_audit(&state, &session, Some(addr.ip()), ua_header, "RESTORE", "full_system").await;

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if field.name() == Some("backup") {
            let data = field.bytes().await.unwrap_or_default();
            
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

            return (StatusCode::OK, Json(json!({"status": "System restored. Please refresh."})));
        }
    }
    
    (StatusCode::BAD_REQUEST, Json(json!({"error": "No backup file provided"})))
}

async fn proxy_request(url: &str, body: Option<Value>) -> (StatusCode, Json<Value>) {
    let client = reqwest::Client::new();
    let req = if let Some(b) = body { 
        client.post(url).json(&b) 
    } else { 
        client.get(url) 
    };
    
    match req.send().await {
        Ok(res) => {
            let status = res.status();
            let data = res.json::<Value>().await.unwrap_or_else(|_| json!({"error": "Invalid node response"}));
            if !status.is_success() {
                println!("⚠️ [WEB] Proxy Error [{}]: {:?}", status, data);
            }
            let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status_code, Json(data))
        },
        Err(e) => {
            println!("❌ [WEB] Node Unreachable: {}", e);
            (StatusCode::BAD_GATEWAY, Json(json!({"error": "Node unreachable"})))
        }
    }
}