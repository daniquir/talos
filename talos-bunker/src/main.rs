use axum::{
    routing::post,
    Router,
};
use std::env;
use std::net::SocketAddr;
use tower_http::limit::RequestBodyLimitLayer;

mod gpg;
use crate::gpg::process_gpg;

#[derive(Clone)]
pub struct AppState {}

#[tokio::main]
async fn main() {
    // Ensure GPG_ID is set for security
    let gpg_id = env::var("GPG_ID").expect("GPG_ID environment variable must be set for security");
    if gpg_id.is_empty() {
        panic!("GPG_ID cannot be empty");
    }
    println!(" [BUNKER] GPG_ID configured: {}", gpg_id);

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