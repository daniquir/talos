mod gpg;
mod init;

use axum::{routing::post, Router};
use std::env;
use crate::gpg::process_gpg;
use crate::init::init_bunker;

#[tokio::main]
async fn main() {
    // Fix permissions for GPG home to silence warnings and prevent errors
    // This is necessary because volume mounts might have broad permissions
    let _ = std::process::Command::new("chmod").args(["700", "/root/.gnupg"]).status();

    init_bunker().await;
    let port = env::var("PORT").unwrap_or_else(|_| "5000".to_string());
    let addr = format!("0.0.0.0:{}", port);

    let app = Router::new().route("/process", post(process_gpg));
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("🛡️ GPG Bunker active and shielded on port {}", port);
    axum::serve(listener, app).await.unwrap();
}