mod models;
mod handlers;
mod init;
mod config;

use axum::{routing::{get, post}, Router};
use std::env;
use crate::handlers::{list_tree, decrypt_secret, encrypt_and_save, delete_entry, storage_health_check, download_backup, restore_backup, create_category, unlock_bunker, initialize_bunker, import_bunker_key, backup_bunker_key};
use crate::init::init_storage;

#[tokio::main]
async fn main() {
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
        .route("/api/health", get(storage_health_check));

    let port = env::var("PORT").unwrap_or_else(|_| "4000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("🌉 Storage Bridge active on port {}", port);
    axum::serve(listener, app).await.unwrap();
}
