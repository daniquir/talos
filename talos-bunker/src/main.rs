use axum::{
    routing::post,
    Router,
};
use std::net::SocketAddr;

mod gpg;
use crate::gpg::process_gpg;

#[derive(Clone)]
pub struct AppState {}

#[tokio::main]
async fn main() {
    let state = AppState {};

    let app = Router::new()
        .route("/process", post(process_gpg))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 5000));
    println!("🛡️ TALOS-BUNKER ONLINE // PORT: 5000");
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}