use axum::Json;
use axum::extract::Multipart;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use serde_json::{json, Value};
use std::env;

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

pub async fn proxy_decrypt(Json(body): Json<Value>) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying DECRYPT"); }
    proxy_request(&format!("{}/api/decrypt", storage_url), Some(body)).await
}

pub async fn proxy_save(Json(body): Json<Value>) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying SAVE"); }
    proxy_request(&format!("{}/api/save", storage_url), Some(body)).await
}

pub async fn proxy_delete(Json(body): Json<Value>) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying DELETE"); }
    proxy_request(&format!("{}/api/delete", storage_url), Some(body)).await
}

pub async fn proxy_create_category(Json(body): Json<Value>) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying CREATE CATEGORY"); }
    proxy_request(&format!("{}/api/create_category", storage_url), Some(body)).await
}

pub async fn proxy_backup() -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying BACKUP download"); }
    
    let client = reqwest::Client::new();
    match client.get(format!("{}/api/backup", storage_url)).send().await {
        Ok(res) => {
            let bytes = res.bytes().await.unwrap_or_default();
            (StatusCode::OK, [(header::CONTENT_TYPE, "application/zip")], bytes)
        },
        Err(_) => {
            (StatusCode::BAD_GATEWAY, [(header::CONTENT_TYPE, "text/plain")], axum::body::Bytes::from("Error fetching backup"))
        }
    }
}

pub async fn proxy_restore(mut multipart: Multipart) -> impl IntoResponse {
    let storage_url = env::var("STORAGE_URL").unwrap_or_else(|_| "http://talos-storage:4000".to_string());
    if is_debug() { println!("--> [WEB] Proxying RESTORE upload"); }

    // Reconstruct multipart form for reqwest
    let mut form = reqwest::multipart::Form::new();
    
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if field.name() == Some("backup") {
            let file_name = field.file_name().unwrap_or("backup.zip").to_string();
            let data = field.bytes().await.unwrap_or_default();
            let part = reqwest::multipart::Part::bytes(data.to_vec()).file_name(file_name);
            form = form.part("backup", part);
        }
    }

    let client = reqwest::Client::new();
    match client.post(format!("{}/api/restore", storage_url))
        .multipart(form)
        .send().await 
    {
        Ok(res) => {
             let status = res.status();
             let data = res.json::<Value>().await.unwrap_or(json!({"status": "ok"}));
             let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
             (status_code, Json(data))
        },
        Err(e) => {
             println!("❌ [WEB] Node Unreachable: {}", e);
             (StatusCode::BAD_GATEWAY, Json(json!({"error": "Node unreachable"})))
        }
    }
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