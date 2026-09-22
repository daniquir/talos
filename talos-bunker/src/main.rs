use axum::{
    routing::post,
    Router,
};
use std::env;
use std::net::SocketAddr;
use tower_http::limit::RequestBodyLimitLayer;

mod gpg;
mod user_vault;
use crate::gpg::process_gpg;

#[derive(Clone)]
pub struct AppState {}

#[tokio::main]
async fn main() {
    // Legacy GPG_ID still required for single-tenant / default identity template.
    let gpg_id = env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string());
    if gpg_id.is_empty() {
        panic!("GPG_ID cannot be empty");
    }
    println!(" [BUNKER] GPG_ID configured: {}", gpg_id);
    println!(
        " [BUNKER] MULTIUSER={} CUSTODY={}",
        env::var("MULTIUSER").unwrap_or_else(|_| "true".into()),
        env::var("TALOS_CUSTODY_MODE").unwrap_or_else(|_| "strict".into())
    );

    // Ensure SHARED_SECRET is set for authentication
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_else(|_| {
        println!("⚠️  WARNING: SHARED_SECRET not set, using default (INSECURE!)");
        "changeme_in_production".to_string()
    });
    if shared_secret == "changeme_in_production" {
        println!("⚠️  WARNING: Using default SHARED_SECRET - CHANGE IN PRODUCTION!");
    } else {
        println!(" [BUNKER] Shared secret configured");
    }

    // Optional boot-time operator KEK for convenience mode (dev only recommended)
    if crate::user_vault::is_convenience() {
        if let Ok(kek) = env::var("TALOS_OPERATOR_KEK") {
            if !kek.is_empty() {
                match crate::user_vault::operator_unseal(&kek) {
                    Ok(()) => println!(" [BUNKER] Operator KEK loaded from TALOS_OPERATOR_KEK"),
                    Err(e) => println!("⚠️  [BUNKER] Failed to load operator KEK: {}", e),
                }
            }
        }
    }

    let state = AppState {};

    let app = Router::new()
        .route("/process", post(process_gpg))
        .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024)) // 10MB limit
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 5000));
    println!(" TALOS-BUNKER ONLINE // PORT: 5000");
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}