use axum::{Json, extract::Path};
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

pub async fn decrypt_secret(Json(req): Json<ActionRequest>) -> (StatusCode, Json<String>) {
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
            let data: serde_json::Value = res.json().await.unwrap_or(serde_json::json!({"result": "Data error"}));
            (StatusCode::OK, Json(data["result"].as_str().unwrap_or("Unknown error").to_string()))
        },
        _ => (StatusCode::INTERNAL_SERVER_ERROR, Json("Error: Bunker unavailable or decryption failed".to_string()))
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
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(file_path, armored_gpg).unwrap();
            
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
                let outpath = StdPath::new(store_path).join(file.mangled_name());

                if file.name().ends_with('/') {
                    fs::create_dir_all(&outpath).unwrap();
                } else {
                    if let Some(p) = outpath.parent() {
                        if !p.exists() { fs::create_dir_all(p).unwrap(); }
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

    let bunker_ok = match bunker_res {
        Ok(res) => res.status().is_success(),
        Err(_) => false,
    };

    Json(json!({
        "storage": true, // Storage is reachable if this code executes
        "bunker": bunker_ok
    }))
}