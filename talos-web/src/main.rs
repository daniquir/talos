mod handlers;
mod db;
mod auth;
mod state;
mod settings;
mod oidc;
mod user_proxy;

use axum::{routing::{get, post, put}, Router, middleware};
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};
use time::Duration;
use tower_http::services::ServeDir;
use tower_http::compression::CompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;
use std::{env, net::SocketAddr, collections::HashMap, sync::{Arc, Mutex}};
use crate::handlers::{get_version, proxy_list_tree, proxy_decrypt, proxy_save, proxy_delete, proxy_backup, proxy_restore, health_check, proxy_create_category, get_audit_logs, proxy_initialize, proxy_match, proxy_reindex};
use crate::db::init_db;
use crate::auth::{
    get_auth_status, login, logout, require_auth, proxy_import_key, proxy_backup_key, issue_token,
    issue_token_oidc, oidc_login, oidc_callback, operator_unseal,
};
use crate::settings::{ensure_settings_schema, get_settings, put_settings};
use crate::state::AppState;
use crate::oidc::{JwksCache, OidcConfig};

#[tokio::main]
async fn main() {
    let pool = init_db().await;
    ensure_settings_schema(&pool).await;

    let oidc = OidcConfig::from_env();
    if oidc.enabled {
        println!("🔐 [SYSTEM] OIDC enabled issuer={}", oidc.issuer);
    } else {
        println!("ℹ️  [SYSTEM] OIDC disabled (MULTIUSER=false or OIDC_ISSUER unset) — legacy master-key auth");
    }

    let app_state = AppState {
        pool: pool.clone(),
        rate_limiter: Arc::new(Mutex::new(HashMap::new())),
        match_rate_limiter: Arc::new(Mutex::new(HashMap::new())),
        token_store: Arc::new(Mutex::new(HashMap::new())),
        oidc,
        jwks: JwksCache::default(),
    };

    let debug_mode = env::var("DEBUG").unwrap_or_default() == "true";
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(!debug_mode)
        .with_http_only(true)
        .with_same_site(tower_sessions::cookie::SameSite::Lax) // Lax needed for OIDC redirect back
        .with_expiry(Expiry::OnInactivity(Duration::hours(2)));

    if debug_mode {
        println!("⚠️  [SYSTEM] DEBUG MODE: HTTP session cookies enabled (dev only).");
    } else {
        println!("🔒 [SYSTEM] SECURE MODE ACTIVE: Authentication required.");
    }

    let api_router = Router::new()
        .route("/api/tree", get(proxy_list_tree))
        .route("/api/match", get(proxy_match))
        .route("/api/match/reindex", post(proxy_reindex))
        .route("/api/decrypt", post(proxy_decrypt))
        .route("/api/save", post(proxy_save))
        .route("/api/delete", post(proxy_delete))
        .route("/api/backup", get(proxy_backup))
        .route("/api/restore", post(proxy_restore))
        .route("/api/create_category", post(proxy_create_category))
        .route("/api/audit", get(get_audit_logs))
        .route("/api/settings", put(put_settings))
        .route("/api/auth/operator/unseal", post(operator_unseal))
        .route_layer(middleware::from_fn_with_state(app_state.clone(), require_auth));

    let app = Router::new()
        .route("/api/auth/status", get(get_auth_status))
        .route("/api/auth/login", post(login))
        .route("/api/auth/token", post(issue_token))
        .route("/api/auth/token/oidc", post(issue_token_oidc))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/oidc/login", get(oidc_login))
        .route("/api/auth/oidc/callback", get(oidc_callback))
        .route("/api/initialize/import", post(proxy_import_key))
        .route("/api/auth/backup-key", get(proxy_backup_key))
        .route("/api/version", get(get_version))
        .route("/api/health", get(health_check))
        .route("/api/initialize", post(proxy_initialize))
        .route("/api/settings", get(get_settings))
        .merge(api_router)
        .fallback_service(ServeDir::new("./static"))
        .layer(CompressionLayer::new())
        .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024))
        .layer(session_layer)
        .with_state(app_state);

    let port = env::var("PORT").unwrap_or_else(|_| "3000".to_string()).parse().unwrap();
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("🚀 TALOS-WEB ONLINE // PORT: {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
}
