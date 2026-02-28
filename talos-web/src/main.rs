mod handlers;
mod db;
mod auth;
mod state;

use axum::{routing::{get, post}, Router, middleware};
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};
use time::Duration;
use tower_http::services::ServeDir;
use tower_http::compression::CompressionLayer;
use std::{env, net::SocketAddr};
use crate::handlers::{get_version, proxy_list_tree, proxy_decrypt, proxy_save, proxy_delete, proxy_backup, proxy_restore, health_check, proxy_create_category, get_audit_logs, proxy_initialize};
use crate::db::init_db;
use crate::auth::{get_auth_status, login, logout, require_auth, proxy_import_key, proxy_backup_key};
use crate::state::AppState;

#[tokio::main]
async fn main() {
    // 1. Initialize Database
    let pool = init_db().await;

    // 2. Create application state
    let app_state = AppState {
        pool: pool.clone(),
    };

    // 3. Configure session layer
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false) // In production, this should be true if using HTTPS
        .with_expiry(Expiry::OnInactivity(Duration::minutes(15)));

    println!("🔒 [SYSTEM] SECURE MODE ACTIVE: Authentication required.");

    // API routes that require authentication
    let api_router = Router::new()
        .route("/api/tree", get(proxy_list_tree))
        .route("/api/decrypt", post(proxy_decrypt))
        .route("/api/save", post(proxy_save))
        .route("/api/delete", post(proxy_delete))
        .route("/api/backup", get(proxy_backup))
        .route("/api/restore", post(proxy_restore))
        .route("/api/create_category", post(proxy_create_category))
        .route("/api/audit", get(get_audit_logs))
        .route_layer(middleware::from_fn_with_state(app_state.clone(), require_auth));

    let app = Router::new()
        // Authentication routes
        .route("/api/auth/status", get(get_auth_status))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        // Public routes
        .route("/api/version", get(get_version))
        .route("/api/health", get(health_check))
        .route("/api/initialize", post(proxy_initialize))
        .route("/api/initialize/import", post(proxy_import_key))
        .route("/api/auth/backup-key", get(proxy_backup_key))
        // Merge authenticated API routes
        .merge(api_router)
        // Serve static files as a fallback
        .fallback_service(ServeDir::new("./static"))
        // Apply layers (middleware)
        .layer(CompressionLayer::new())
        .layer(session_layer)
        .with_state(app_state);

    let port = env::var("PORT").unwrap_or_else(|_| "3000".to_string()).parse().unwrap();
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("🚀 TALOS-WEB ONLINE // PORT: {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
}