mod models;
mod handlers;
mod init;
mod config;
mod url_index;
mod user_ctx;

use axum::{routing::{get, post}, Router};
use std::env;
use tower_http::limit::RequestBodyLimitLayer;
use crate::handlers::{list_tree, decrypt_secret, encrypt_and_save, delete_entry, storage_health_check, download_backup, restore_backup, create_category, unlock_bunker, initialize_bunker, import_bunker_key, backup_bunker_key, match_secrets, rebuild_url_index, operator_unseal, operator_status, unlock_wrapped};
use crate::init::init_storage;

#[tokio::main]
async fn main() {
    // Ensure GPG_ID is set for security
    let gpg_id = env::var("GPG_ID").expect("GPG_ID environment variable must be set for security");
    if gpg_id.is_empty() {
        panic!("GPG_ID cannot be empty");
    }
    println!("🔒 [STORAGE] GPG_ID configured: {}", gpg_id);
    
    // Ensure SHARED_SECRET is set for authentication
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_else(|_| {
        println!("⚠️  WARNING: SHARED_SECRET not set, using default (INSECURE!)");
        "changeme_in_production".to_string()
    });
    if shared_secret == "changeme_in_production" {
        println!("⚠️  WARNING: Using default SHARED_SECRET - CHANGE IN PRODUCTION!");
    } else {
        println!("🔒 [STORAGE] Shared secret configured");
    }

    println!("🌉 Initializing TALOS Storage...");
    init_storage().await;

    let app = Router::new()
        .route("/api/tree", get(list_tree))
        .route("/api/match", get(match_secrets))
        .route("/api/match/reindex", post(rebuild_url_index))
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
        .route("/api/unlock/wrapped", post(unlock_wrapped))
        .route("/api/operator/unseal", post(operator_unseal))
        .route("/api/operator/status", get(operator_status))
        .route("/api/health", get(storage_health_check))
        .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024)); // 10MB limit

    let port = env::var("PORT").unwrap_or_else(|_| "4000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("🌉 Storage Bridge active on port {}", port);
    axum::serve(listener, app).await.unwrap();
}
