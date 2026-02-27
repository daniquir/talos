mod handlers;

use axum::{routing::{get, post}, Router};
use tower_http::services::ServeDir;
use std::{env, net::SocketAddr};
use crate::handlers::{get_version, proxy_list_tree, proxy_decrypt, proxy_save, proxy_delete, proxy_backup, proxy_restore, health_check, proxy_create_category};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/api/version", get(get_version))
        .route("/api/tree", get(proxy_list_tree))
        .route("/api/decrypt", post(proxy_decrypt))
        .route("/api/save", post(proxy_save))
        .route("/api/delete", post(proxy_delete))
        .route("/api/backup", get(proxy_backup))
        .route("/api/restore", post(proxy_restore))
        .route("/api/create_category", post(proxy_create_category))
        .route("/api/health", get(health_check))
        .fallback_service(ServeDir::new("./static"));

    let port = env::var("PORT").unwrap_or_else(|_| "3000".to_string()).parse().unwrap();
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("🚀 TALOS-WEB ONLINE // PORT: {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}