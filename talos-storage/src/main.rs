mod models;
mod handlers;
mod init;
mod config;

use axum::{routing::{get, post}, Router};
use std::env;
use tower_http::limit::RequestBodyLimitLayer;
use crate::handlers::{list_tree, decrypt_secret, encrypt_and_save, delete_entry, storage_health_check, download_backup, restore_backup, create_category, unlock_bunker, initialize_bunker, import_bunker_key, backup_bunker_key};
use crate::init::init_storage;

#[tokio::main]
async fn main() {
    // Ensure GPG_ID is set for security
    let gpg_id = env::var("GPG_ID").expect("GPG_ID environment variable must be set for security");
    if gpg_id.is_empty() {
        panic!("GPG_ID cannot be empty");
    }
    println!("🔒 [STORAGE] GPG_ID configured: {}", gpg_id);

    println!("🌉 Initializing TALOS Storage...");
    init_storage().await;

    let app = Router::new()
        .route("/api/tree", get(list_tree))
        .route("/api/decrypt", post(decrypt_secret))
        .route("/api/save", post(encrypt_and_save))
        .route("/api/delete", post(delete_entry))
        .route("/api/backup", get(download_backup))
        .route("/api/restore", post(restore_backup))
        .route("/api/create_category", post(create_category))
        .route("/api/initialize", post(initialize_bunker))
        .route("/api/initialize/import", post(import_bunker_key))
        .route("/api/backup/key", get(backup_bunker_key))
        .route("/api/unlock", post(unlock_bunker))
        .route("/api/health", get(storage_health_check))
        .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024)); // 10MB limit

    let port = env::var("PORT").unwrap_or_else(|_| "4000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("🌉 Storage Bridge active on port {}", port);
    axum::serve(listener, app).await.unwrap();
}
