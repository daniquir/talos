use axum::Json;
use axum::extract::Multipart;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use serde::Serialize;
use serde_json::{json, Value};
use std::{env, fs, io::{self, Cursor, Write}, path::Path as StdPath};
use crate::models::{ActionRequest, BunkerTask};
use crate::config::{CONFIG, DEBUG_MODE, STORE_PATH};
use zip::write::FileOptions;

#[derive(Serialize)]
pub struct TreeNode {
    name: String,
    path: String,
    is_dir: bool,
    children: Option<Vec<TreeNode>>,
}

fn build_tree_recursive(base_path: &str, current_path: &str) -> Vec<TreeNode> {
    let full_path = StdPath::new(base_path).join(current_path);
    let mut nodes = Vec::new();

    if let Ok(read_dir) = fs::read_dir(full_path) {
        for entry in read_dir.flatten() {
            let file_name = entry.file_name().into_string().unwrap();
            // Filter out git and config files, but allow files that are just ".gpg"
            if file_name == ".git" || file_name == ".gpg-id" || file_name == ".gitkeep" { continue; }

            let is_dir = entry.path().is_dir();
            let path_str = StdPath::new(current_path).join(&file_name).to_str().unwrap().to_string();
            
            let children = if is_dir {
                Some(build_tree_recursive(base_path, &path_str))
            } else {
                None
            };

            nodes.push(TreeNode {
                name: file_name.replace(".gpg", ""),
                path: path_str.replace(".gpg", ""),
                is_dir,
                children,
            });
        }
    }
    nodes.sort_by(|a, b| a.name.cmp(&b.name));
    nodes
}

pub async fn list_tree() -> Json<Vec<TreeNode>> {
    if *DEBUG_MODE { println!("--> [STORAGE] LIST TREE request"); }
    let root_path = STORE_PATH.as_str();
    let nodes = build_tree_recursive(root_path, "");
    Json(nodes)
}

pub async fn decrypt_secret(Json(req): Json<ActionRequest>) -> (StatusCode, Json<Value>) {
    let file_path = format!("{}/{}.gpg", &*STORE_PATH, req.path);
    if *DEBUG_MODE { println!("--> [STORAGE] DECRYPT request for: {}", req.path); }
    let encrypted_content = fs::read_to_string(file_path).unwrap_or_else(|_| "".into());
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());

    let client = reqwest::Client::new();
    match client.post(format!("{}/process", bunker_url))
        .json(&BunkerTask {
            payload: encrypted_content,
            mode: "decrypt".to_string(),
        })
        .send().await 
    {
        Ok(res) if res.status().is_success() => {
            let data: Value = res.json().await.unwrap_or(json!({"result": "Data error"}));
            let mut decrypted = data["result"].as_str().unwrap_or("").to_string();

            // If revealing the secret is not explicitly requested, we obfuscate it.
            if !req.reveal.unwrap_or(false) {
                if let Some(first_line_end) = decrypted.find('\n') {
                    decrypted.replace_range(..first_line_end, "__TALOS_HIDDEN_SECRET__");
                } else {
                    decrypted = "__TALOS_HIDDEN_SECRET__".to_string();
                }
            }
            (StatusCode::OK, Json(json!(decrypted)))
        },
        _ => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!("Error: Bunker unavailable or decryption failed")))
    }
}

pub async fn encrypt_and_save(Json(req): Json<ActionRequest>) -> (StatusCode, Json<Value>) {
    if *DEBUG_MODE { println!("--> [STORAGE] SAVE request for: {}", req.path); }
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    
    let mut payload = req.content.unwrap_or_default();
    
    let client = reqwest::Client::new();
    
    if payload.starts_with("__TALOS_KEEP_SECRET__") {
        let file_path = format!("{}/{}.gpg", &*STORE_PATH, req.path);
        let encrypted_content = fs::read_to_string(&file_path).unwrap_or_default();
        
        let decrypt_res = client.post(format!("{}/process", bunker_url))
            .json(&BunkerTask {
                payload: encrypted_content,
                mode: "decrypt".to_string(),
            })
            .send().await;
            
        if let Ok(res) = decrypt_res {
            if res.status().is_success() {
                let data: serde_json::Value = res.json().await.unwrap_or(json!({"result": ""}));
                let full_text = data["result"].as_str().unwrap_or("").to_string();
                let old_pass = full_text.split('\n').next().unwrap_or("");
                
                // Replace marker with old password
                payload = payload.replace("__TALOS_KEEP_SECRET__", old_pass);
            }
        }
    }

    let res_result = client.post(format!("{}/process", bunker_url))
        .json(&BunkerTask {
            payload: payload,
            mode: "encrypt".to_string(),
        })
        .send().await;

    match res_result {
        Ok(res) if res.status().is_success() => {
            let data: serde_json::Value = res.json().await.unwrap_or(serde_json::json!({"result": ""}));
            let armored_gpg = data["result"].as_str().unwrap_or("");
            
            if armored_gpg.is_empty() {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Encryption failed"})));
            }

            let file_path = format!("{}/{}.gpg", &*STORE_PATH, req.path);
            if let Some(parent) = std::path::Path::new(&file_path).parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    println!("❌ [STORAGE] Error creating directory: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not create directory"})));
                }
            }
            if let Err(e) = fs::write(&file_path, armored_gpg) {
                println!("❌ [STORAGE] Error writing file: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not write secret to disk"})));
            }
            
            let mut commit_msg = format!("Update secret: {}", req.path);

            if let Some(original_path) = &req.original_path {
                if &req.path != original_path {
                    // This is a move operation
                    let old_file_path = format!("{}/{}.gpg", &*STORE_PATH, original_path);
                    if fs::remove_file(old_file_path).is_ok() {
                        if *DEBUG_MODE { println!("--> [STORAGE] Removed old file for move: {}", original_path); }
                        commit_msg = format!("Move secret from {} to {}", original_path, req.path);
                    }
                }
            }
            
            commit_changes(&commit_msg);
            (StatusCode::OK, Json(json!({"status": "OK"})))
        },
        _ => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "Bunker unavailable"})))
    }
}

pub async fn delete_entry(Json(req): Json<ActionRequest>) -> (StatusCode, Json<Value>) {
    if *DEBUG_MODE { println!("--> [STORAGE] DELETE request for: {}", req.path); }

    let store_path = STORE_PATH.as_str();
    let path_as_dir = StdPath::new(store_path).join(&req.path);
    let path_as_file = StdPath::new(store_path).join(format!("{}.gpg", req.path));

    if path_as_file.is_file() {
        // Attempt to delete as a file
        if fs::remove_file(&path_as_file).is_ok() {
            commit_changes(&format!("Delete secret: {}", req.path));
            (StatusCode::OK, Json(json!({"status": "OK"})))
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not delete file"})))
        }
    } else if path_as_dir.is_dir() {
        // Attempt to delete as a directory
        match fs::read_dir(&path_as_dir) {
            Ok(dir) => {
                // Check if directory contains anything other than .gitkeep
                let non_gitkeep_entries = dir.filter_map(Result::ok)
                    .filter(|e| e.file_name() != ".gitkeep")
                    .count();

                if non_gitkeep_entries == 0 {
                    if fs::remove_dir_all(&path_as_dir).is_ok() {
                        commit_changes(&format!("Delete category: {}", req.path));
                        (StatusCode::OK, Json(json!({"status": "OK"})))
                    } else {
                        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not delete directory"})))
                    }
                } else {
                    (StatusCode::CONFLICT, Json(json!({"error": "Category is not empty. Please remove all secrets and sub-categories first."})))
                }
            }
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not read directory contents"})))
        }
    } else {
        (StatusCode::NOT_FOUND, Json(json!({"error": "Entry not found"})))
    }
}

pub async fn create_category(Json(req): Json<ActionRequest>) -> (StatusCode, Json<Value>) {
    if *DEBUG_MODE { println!("--> [STORAGE] CREATE CATEGORY request for: {}", req.path); }
    let dir_path = format!("{}/{}", &*STORE_PATH, req.path);
    
    if let Err(_) = fs::create_dir_all(&dir_path) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not create directory"})));
    }

    let _ = fs::write(format!("{}/.gitkeep", dir_path), "");
    commit_changes(&format!("Add category: {}", req.path));
    (StatusCode::OK, Json(json!({"status": "OK"})))
}

pub async fn download_backup() -> impl IntoResponse {
    if *DEBUG_MODE { println!("--> [STORAGE] BACKUP request initiated"); }
    let store_path = STORE_PATH.as_str();
    let mut buf = Vec::new();
    
    {
        let mut zip_writer = zip::ZipWriter::new(Cursor::new(&mut buf));
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);

        let walk_dir = walkdir::WalkDir::new(store_path);
        for entry in walk_dir.into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() {
                let name = path.strip_prefix(store_path).unwrap().to_str().unwrap();
                // Ignore git folder
                if name.starts_with(".git") { continue; }
                
                if let Ok(content) = fs::read(path) {
                    let _ = zip_writer.start_file(name, options);
                    let _ = zip_writer.write_all(&content);
                }
            }
        }
        let _ = zip_writer.finish();
    }

    (
        [
            (header::CONTENT_TYPE, "application/zip"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"talos_backup.zip\""),
        ],
        buf,
    )
}

pub async fn restore_backup(mut multipart: Multipart) -> (StatusCode, Json<Value>) {
    if *DEBUG_MODE { println!("--> [STORAGE] RESTORE request initiated"); }
    
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if field.name() == Some("backup") {
            let data = field.bytes().await.unwrap_or_default();
            let reader = Cursor::new(data);
            let mut archive = match zip::ZipArchive::new(reader) {
                Ok(a) => a,
                Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid zip file"}))),
            };

            let store_path = STORE_PATH.as_str();
            
            // Extract files
            for i in 0..archive.len() {
                let mut file = archive.by_index(i).unwrap();
                let outpath = match StdPath::new(store_path).join(file.mangled_name()) {
                    // Path traversal defense
                    path if path.starts_with(store_path) => path,
                    _ => continue,
                };

                if file.name().ends_with('/') {
                    let _ = fs::create_dir_all(&outpath);
                } else {
                    if let Some(p) = outpath.parent() {
                        if !p.exists() { let _ = fs::create_dir_all(p); }
                    }
                    let mut outfile = fs::File::create(&outpath).unwrap();
                    io::copy(&mut file, &mut outfile).unwrap();
                }
            }
            
            commit_changes("Restored from backup");
            return (StatusCode::OK, Json(json!({"status": "Restored successfully"})));
        }
    }
    (StatusCode::BAD_REQUEST, Json(json!({"error": "No backup file provided"})))
}

#[derive(serde::Deserialize)]
pub struct InitializeRequest {
    pub key: String,
}

pub async fn initialize_bunker(Json(req): Json<InitializeRequest>) -> impl IntoResponse {
    if *DEBUG_MODE { println!("--> [STORAGE] INITIALIZE request received"); }
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let client = reqwest::Client::new();

    // Check if bunker is already initialized
    let check_res: serde_json::Value = client.post(format!("{}/process", bunker_url))
        .json(&BunkerTask { payload: "".to_string(), mode: "check".to_string() })
        .send().await.unwrap().json().await.unwrap();
    
    if check_res["result"].as_str() != Some("UNINITIALIZED") {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "System already initialized"})));
    }

    // Send Initialize Command
    let init_res = client.post(format!("{}/process", bunker_url))
        .json(&BunkerTask {
            payload: req.key,
            mode: "initialize".to_string(),
        })
        .send().await;

    match init_res {
        Ok(res) if res.status().is_success() => {
            let data: serde_json::Value = res.json().await.unwrap_or_default();
            if data["result"].as_str() == Some("INITIALIZED") {
                (StatusCode::OK, Json(json!({"status": "initialized"})))
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Bunker initialization failed"})))
            }
        },
        _ => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "Bunker unreachable or init failed"})))
    }
}

#[derive(serde::Deserialize)]
pub struct ImportRequest {
    pub key: String,
    pub passphrase: String,
}

pub async fn import_bunker_key(Json(req): Json<ImportRequest>) -> impl IntoResponse {
    if *DEBUG_MODE { println!("--> [STORAGE] IMPORT KEY request received"); }
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let client = reqwest::Client::new();

    // Send Import Command to Bunker
    // We send the private key block and the passphrase to unlock/verify it
    let import_res = client.post(format!("{}/process", bunker_url))
        .json(&json!({
            "mode": "import",
            "payload": req.key,
            "passphrase": req.passphrase 
        }))
        .send().await;

    match import_res {
        Ok(res) if res.status().is_success() => {
            (StatusCode::OK, Json(json!({"status": "imported"})))
        },
        _ => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Import failed"})))
    }
}

// Global flag to ensure key can only be downloaded once per session/boot
static mut KEY_DOWNLOADED: bool = false;

pub async fn backup_bunker_key() -> impl IntoResponse {
    unsafe {
        if KEY_DOWNLOADED {
            return (StatusCode::GONE, Json(json!({"error": "Key already downloaded. Access revoked."}))).into_response();
        }
    }

    if *DEBUG_MODE { println!("--> [STORAGE] BACKUP KEY request"); }
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let client = reqwest::Client::new();

    // Request export from Bunker
    let res = client.post(format!("{}/process", bunker_url))
        .json(&json!({ "mode": "export_key", "payload": "" }))
        .send().await;

    match res {
        Ok(response) if response.status().is_success() => {
            let data: Value = response.json().await.unwrap_or_default();
            let key_content = data["result"].as_str().unwrap_or("").to_string();
            
            // Mark as downloaded to prevent future access
            unsafe { KEY_DOWNLOADED = true; }
            
            (StatusCode::OK, key_content).into_response()
        },
        _ => (StatusCode::BAD_GATEWAY, Json(json!({"error": "Failed to export key"}))).into_response()
    }
}

#[derive(serde::Deserialize)]
pub struct UnlockRequest {
    pub key: String,
}

pub async fn unlock_bunker(Json(req): Json<UnlockRequest>) -> impl IntoResponse {
    if *DEBUG_MODE { println!("--> [STORAGE] UNLOCK request received"); }
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let client = reqwest::Client::new();

    // 1. Send Unlock Command (Inject Key into Bunker RAM)
    let unlock_res = client.post(format!("{}/process", bunker_url))
        .json(&BunkerTask {
            payload: req.key.clone(),
            mode: "unlock".to_string(),
        })
        .send().await;

    if unlock_res.is_err() || !unlock_res.unwrap().status().is_success() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "Bunker unreachable"})));
    }

    // 2. Verify Key Validity (The "True Master Key" Check)
    // We attempt to encrypt and then decrypt a canary string. 
    // If the key is wrong, GPG will fail at the encryption or decryption step.
    let test_payload = "TALOS_VERIFY_SEQ";
    
    // A. Encrypt
    let enc_res = client.post(format!("{}/process", bunker_url))
        .json(&BunkerTask {
            payload: test_payload.to_string(),
            mode: "encrypt".to_string(),
        })
        .send().await;

    let encrypted = match enc_res {
        Ok(res) if res.status().is_success() => {
            let data: serde_json::Value = res.json().await.unwrap_or_default();
            data["result"].as_str().unwrap_or("").to_string()
        },
        _ => return (StatusCode::UNAUTHORIZED, Json(json!({"error": "Key rejected (Encryption failed)"})))
    };

    // B. Decrypt
    let dec_res = client.post(format!("{}/process", bunker_url))
        .json(&BunkerTask {
            payload: encrypted,
            mode: "decrypt".to_string(),
        })
        .send().await;

    let decrypted = match dec_res {
        Ok(res) if res.status().is_success() => {
            let data: serde_json::Value = res.json().await.unwrap_or_default();
            data["result"].as_str().unwrap_or("").to_string()
        },
        _ => return (StatusCode::UNAUTHORIZED, Json(json!({"error": "Key rejected (Decryption failed)"})))
    };

    if decrypted.trim() == test_payload {
        (StatusCode::OK, Json(json!({"status": "unlocked"})))
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid Master Key"})))
    }
}

fn commit_changes(msg: &str) {
    let store_path = STORE_PATH.as_str();
    
    if CONFIG.backend.r#type == "git" {
        // Only run git commands if backend is git
        let _ = std::process::Command::new("git").args(["-C", store_path, "add", "."]).status();
        let _ = std::process::Command::new("git").args(["-C", store_path, "commit", "-m", msg]).status();
        
        if *DEBUG_MODE { println!("--> [STORAGE] Pushing changes to remote git..."); }
        let _ = std::process::Command::new("git").args(["-C", store_path, "push", "origin", "HEAD"]).status();
    } else {
        if *DEBUG_MODE { println!("--> [STORAGE] Local change recorded (No Git configured). Backup available at: {}", store_path); }
    }
}

// Health check handler to verify connectivity with the Bunker
pub async fn storage_health_check() -> Json<Value> {
    let client = reqwest::Client::new();
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    
    // Storage communicates with Bunker over the isolated private network
    let bunker_res = client.post(format!("{}/process", bunker_url))
        .json(&json!({"payload":"", "mode":"check"}))
        .send().await;

    let bunker_status = match bunker_res {
        Ok(res) if res.status().is_success() => {
            let data: serde_json::Value = res.json().await.unwrap_or(json!({"result": "ERROR"}));
            data["result"].as_str().unwrap_or("ERROR").to_string()
        },
        _ => "OFFLINE".to_string(),
    };
    
    Json(json!({
        "storage": true, // Storage is reachable if this code executes
        "bunker": bunker_status
    }))
}