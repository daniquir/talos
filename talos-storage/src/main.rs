use axum::{routing::{get, post}, Json, Router, extract::Path};
use serde::{Deserialize, Serialize};
use std::{env, fs};
use reqwest;

#[derive(Serialize)]
struct PassEntry { name: String, is_dir: bool }

#[derive(Deserialize)]
struct ActionRequest {
    path: String,
    passphrase: String,
    content: Option<String>,
}

#[derive(Serialize)]
struct BunkerTask {
    payload: String,
    passphrase: String,
    mode: String,
}

async fn list_entries(Path(path): Path<String>) -> Json<Vec<PassEntry>> {
    let base = "/home/talosuser/.password-store";
    let full_path = if path.is_empty() { base.to_string() } else { format!("{}/{}", base, path) };
    
    let mut entries = Vec::new();
    if let Ok(read_dir) = fs::read_dir(full_path) {
        for entry in read_dir.flatten() {
            let name = entry.file_name().into_string().unwrap();
            if name.starts_with('.') { continue; }
            let is_dir = entry.path().is_dir();
            entries.push(PassEntry { name: name.replace(".gpg", ""), is_dir });
        }
    }
    Json(entries)
}

async fn decrypt_secret(Json(req): Json<ActionRequest>) -> Json<String> {
    let file_path = format!("/home/talosuser/.password-store/{}.gpg", req.path);
    let encrypted_content = fs::read_to_string(file_path).unwrap_or_else(|_| "".into());

    let client = reqwest::Client::new();
    let res = client.post("http://talos-bunker:5000/process")
        .json(&BunkerTask {
            payload: encrypted_content,
            passphrase: req.passphrase,
            mode: "decrypt".to_string(),
        })
        .send().await.unwrap();

    let data: serde_json::Value = res.json().await.unwrap();
    Json(data["result"].as_str().unwrap_or("Error en búnker").to_string())
}

async fn encrypt_and_save(Json(req): Json<ActionRequest>) -> Json<String> {
    let client = reqwest::Client::new();
    let res = client.post("http://talos-bunker:5000/process")
        .json(&BunkerTask {
            payload: req.content.unwrap(),
            passphrase: req.passphrase,
            mode: "encrypt".to_string(),
        })
        .send().await.unwrap();

    let data: serde_json::Value = res.json().await.unwrap();
    let armored_gpg = data["result"].as_str().unwrap();

    let file_path = format!("/home/talosuser/.password-store/{}.gpg", req.path);
    // Asegurar que la carpeta existe antes de escribir
    if let Some(parent) = std::path::Path::new(&file_path).parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(file_path, armored_gpg).unwrap();
    
    Json("OK".to_string())
}

async fn init_storage() {
    let gpg_id = env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string());
    let store_path = "/home/talosuser/.password-store";
    let gpg_id_file = format!("{}/.gpg-id", store_path);

    if !std::path::Path::new(&gpg_id_file).exists() {
        println!("📦 Inicializando bóveda de contraseñas con ID: {}", gpg_id);
        fs::create_dir_all(store_path).unwrap();
        fs::write(gpg_id_file, gpg_id).unwrap();
    }
}

// Handler específico para la raíz que no recibe parámetros
async fn list_root() -> Json<Vec<PassEntry>> {
    list_entries(Path("".to_string())).await
}

// Y el handler correspondiente:
async fn storage_health_check() -> Json<Value> {
    let client = reqwest::Client::new();
    
    // El Storage sí ve al Bunker en la net_private
    let bunker_res = client.post("http://talos-bunker:5000/process")
        .json(&json!({"payload":"", "passphrase":"", "mode":"check"}))
        .send().await;

    Json(json!({
        "storage": true, // Si este código corre, el storage está vivo
        "bunker": bunker_res.is_ok()
    }))
}

#[tokio::main]
async fn main() {
    init_storage().await;

    let app = Router::new()
        // Eliminamos el closure |_| y usamos la función directa
        .route("/api/list/", get(list_root)) 
        .route("/api/list/*path", get(list_entries))
        .route("/api/decrypt", post(decrypt_secret))
        .route("/api/encrypt", post(encrypt_and_save))
        .route("/api/health", get(storage_health_check));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4000").await.unwrap();
    println!("🌉 Storage Bridge activo en puerto 4000");
    axum::serve(listener, app).await.unwrap();
}
