use axum::{routing::{get, post}, Json, Router, extract::Path};
use serde_json::{json, Value};
use tower_http::services::ServeDir;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/api/list/", get(proxy_list_root))
        .route("/api/list/*path", get(proxy_list))
        .route("/api/decrypt", post(proxy_decrypt))
        .route("/api/save", post(proxy_save))
        .route("/api/health", get(health_check))
        .fallback_service(ServeDir::new("./static"));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("🚀 TALOS-WEB ONLINE // PORT: {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health_check() -> Json<Value> {
    let client = reqwest::Client::new();
    
    // Check Storage Bridge
    let storage_res = client.get("http://talos-storage:4000/api/list/").send().await;
    
    // Check Bunker via Storage Proxy (o intento directo de handshake)
    let bunker_res = client.post("http://talos-bunker:5000/process")
        .json(&json!({"payload":"", "passphrase":"", "mode":"check"}))
        .send().await;

    Json(json!({
        "storage": storage_res.is_ok(),
        "bunker": bunker_res.is_ok()
    }))
}

async fn proxy_list_root() -> Json<Value> {
    proxy_request("http://talos-storage:4000/api/list/", None).await
}

async fn proxy_list(Path(path): Path<String>) -> Json<Value> {
    proxy_request(&format!("http://talos-storage:4000/api/list/{}", path), None).await
}

async fn proxy_decrypt(Json(body): Json<Value>) -> Json<Value> {
    proxy_request("http://talos-storage:4000/api/decrypt", Some(body)).await
}

async fn proxy_save(Json(body): Json<Value>) -> Json<Value> {
    proxy_request("http://talos-storage:4000/api/save", Some(body)).await
}

async fn proxy_request(url: &str, body: Option<Value>) -> Json<Value> {
    let client = reqwest::Client::new();
    let req = if let Some(b) = body { 
        client.post(url).json(&b) 
    } else { 
        client.get(url) 
    };
    
    match req.send().await {
        Ok(res) => {
            let data = res.json::<Value>().await.unwrap_or(json!({"error": "Respuesta inválida del nodo"}));
            Json(data)
        },
        Err(_) => Json(json!({"error": "Nodo inalcanzable"}))
    }
}