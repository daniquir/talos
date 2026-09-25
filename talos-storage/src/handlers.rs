use axum::Json;
use axum::extract::Multipart;
use axum::http::{StatusCode, header, HeaderMap};
use axum::response::IntoResponse;
use serde::Serialize;
use serde_json::{json, Value};
use std::{env, fs, io::{self, Cursor, Write}, path::Path as StdPath, sync::Arc};
use crate::models::{ActionRequest, BunkerTask};
use crate::config::{CONFIG, DEBUG_MODE, STORE_PATH};
use crate::user_ctx::{ensure_user_store, extract_user_sub, require_user_sub};
use zip::write::FileOptions;
use chrono::Utc;
use base64::{Engine as _, engine::general_purpose};
use sha2::{Sha256, Digest};
use hmac::{Hmac, Mac};
use hex;

type HmacSha256 = Hmac<Sha256>;

fn bunker_task(mode: &str, payload: String, user_sub: Option<&str>) -> BunkerTask {
    BunkerTask {
        payload,
        mode: mode.to_string(),
        signature: None,
        user_sub: user_sub.map(|s| s.to_string()),
        passphrase: None,
    }
}

fn log_audit_event(action: &str, status: &str, details: &str) {
    let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    eprintln!("[AUDIT {}] ACTION={} STATUS={} DETAILS={}", timestamp, action, status, details);
}

fn verify_signature(result: &str, signature: &str) -> bool {
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    let mut mac = HmacSha256::new_from_slice(shared_secret.as_bytes()).unwrap();
    mac.update(result.as_bytes());
    let expected_signature = hex::encode(mac.finalize().into_bytes());
    ct_eq(&expected_signature, signature)
}

fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Resolve vault identity from verified HMAC headers only.
/// Never trust `user_sub` from JSON bodies (defense-in-depth against spoofing).
fn vault_user_from_headers(headers: &HeaderMap) -> Result<Option<String>, StatusCode> {
    require_user_sub(headers)
}

// Validate and sanitize path to prevent path traversal attacks
fn validate_path(path: &str) -> Result<(), String> {
    // Prevent null bytes
    if path.contains('\0') {
        return Err("Path contains null byte".to_string());
    }

    // Prevent absolute paths
    if StdPath::new(path).is_absolute() {
        return Err("Absolute paths not allowed".to_string());
    }

    // Prevent path traversal
    if path.contains("..") {
        return Err("Path traversal not allowed".to_string());
    }

    // Prevent shell metacharacters
    if path.chars().any(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '$' | '`' | '|' | ';' | '&' | '>' | '<')) {
        return Err("Invalid characters in path".to_string());
    }

    // Limit path length
    if path.len() > 255 {
        return Err("Path too long".to_string());
    }

    Ok(())
}

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
            // Hide VCS / pass / Talos metadata — only dirs and *.gpg secrets belong in the tree
            if file_name == ".git"
                || file_name == ".gpg-id"
                || file_name == ".gitkeep"
                || file_name == ".talos-url-index.json"
                || file_name.starts_with(".talos-")
            {
                continue;
            }

            let is_dir = entry.path().is_dir();
            if !is_dir && !file_name.ends_with(".gpg") {
                continue;
            }
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

pub async fn list_tree(headers: HeaderMap) -> Result<Json<Vec<TreeNode>>, StatusCode> {
    if *DEBUG_MODE { println!("--> [STORAGE] LIST TREE request"); }
    let user = require_user_sub(&headers)?;
    let root_path = ensure_user_store(user.as_deref())?;
    let nodes = build_tree_recursive(&root_path, "");
    Ok(Json(nodes))
}

fn flatten_secret_paths(nodes: &[TreeNode]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for node in nodes {
        if node.is_dir {
            if let Some(ref children) = node.children {
                out.extend(flatten_secret_paths(children));
            }
        } else {
            out.push((node.path.clone(), node.name.clone()));
        }
    }
    out
}

fn normalize_hostname(hostname: &str) -> String {
    let h = hostname.trim().to_lowercase();
    h.strip_prefix("www.").unwrap_or(&h).to_string()
}

/// Extract `hostname` or `hostname:port` (non-default ports kept).
/// Accepts full URLs or bare host[/path] values.
fn authority_key(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let scheme = s
        .split_once("://")
        .map(|(sch, _)| sch.to_lowercase());
    let without_scheme = s.split("://").nth(1).unwrap_or(s);
    // Drop userinfo if present
    let after_at = without_scheme.rsplit('@').next().unwrap_or(without_scheme);
    let hostport = after_at.split('/').next()?.split('?').next()?.split('#').next()?;
    if hostport.is_empty() {
        return None;
    }

    let (hostname, port) = if hostport.starts_with('[') {
        // [IPv6] or [IPv6]:port
        let end = hostport.find(']')?;
        let host = &hostport[1..end];
        let rest = &hostport[end + 1..];
        let port = rest.strip_prefix(':').filter(|p| !p.is_empty());
        (host, port)
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) if !h.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
                (h, Some(p))
            }
            _ => (hostport, None),
        }
    };

    if hostname.is_empty() {
        return None;
    }
    let hostname = normalize_hostname(hostname);

    let keep_port = match port {
        Some(p) => {
            let default = matches!(
                (scheme.as_deref(), p),
                (Some("http"), "80") | (Some("https"), "443") | (None, "80") | (None, "443")
            );
            !default
        }
        None => false,
    };

    if keep_port {
        Some(format!("{}:{}", hostname, port.unwrap()))
    } else {
        Some(hostname)
    }
}

fn hosts_match(query_host: &str, stored_url: &str) -> bool {
    let Some(q) = authority_key(query_host) else {
        return false;
    };
    let Some(s) = authority_key(stored_url) else {
        return false;
    };
    if q == s {
        return true;
    }

    // Subdomain match only when neither side carries an explicit non-default port.
    let (q_host, q_port) = match q.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h, Some(p)),
        _ => (q.as_str(), None),
    };
    let (s_host, s_port) = match s.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h, Some(p)),
        _ => (s.as_str(), None),
    };
    if q_port != s_port {
        return false;
    }
    // Only allow query as a subdomain of the stored host (not the reverse),
    // so evil.example.com does not pull credentials stored for sibling hosts,
    // and a stored entry for evil.bank.com never matches bank.com.
    q_host.ends_with(&format!(".{}", s_host))
}

fn parse_pass_metadata(content: &str) -> (Option<String>, Option<String>) {
    let mut user = None;
    let mut url = None;
    for line in content.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().to_string();
        if value.is_empty() {
            continue;
        }
        match key.trim().to_ascii_lowercase().as_str() {
            "user" | "username" | "login" => user = Some(value),
            "url" | "website" => url = Some(value),
            _ => {}
        }
    }
    (user, url)
}

async fn decrypt_metadata(store_root: &str, path: &str, user_sub: Option<&str>) -> Option<String> {
    let file_path = format!("{}/{}.gpg", store_root, path);
    let encrypted_bytes = fs::read(file_path).ok()?;
    let encrypted_content = general_purpose::STANDARD.encode(&encrypted_bytes);
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();

    let client = reqwest::Client::new();
    let res = client
        .post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", shared_secret)
        .json(&bunker_task("decrypt", encrypted_content, user_sub))
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        return None;
    }
    let data: Value = res.json().await.ok()?;
    data["result"].as_str().map(|s| s.to_string())
}

/// Match secrets by host/URL using the plaintext metadata index (no bunker / no passwords).
pub async fn match_secrets(
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<(StatusCode, Json<Value>), StatusCode> {
    let user = require_user_sub(&headers)?;
    let root = ensure_user_store(user.as_deref())?;
    let host = params.get("host").cloned().unwrap_or_default();
    if host.trim().is_empty() {
        return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": "host query parameter required"}))));
    }
    if *DEBUG_MODE {
        println!("--> [STORAGE] MATCH request host={}", host);
    }

    let mut matches = Vec::new();
    for entry in crate::url_index::all_entries_in(&root) {
        let url_for_match = entry.url.clone().unwrap_or_default();
        let path_hint = entry
            .path
            .split('/')
            .last()
            .unwrap_or("")
            .to_string();

        let matched = (!url_for_match.is_empty() && hosts_match(&host, &url_for_match))
            || hosts_match(&host, &path_hint)
            || hosts_match(&host, &entry.title);

        if matched {
            matches.push(json!({
                "path": entry.path,
                "title": entry.title,
                "username": entry.username,
                "url": entry.url,
            }));
        }
    }

    Ok((StatusCode::OK, Json(json!({ "matches": matches }))))
}

/// Rebuild URL index by decrypting metadata for every secret (requires unsealed bunker).
pub async fn rebuild_url_index(headers: HeaderMap) -> Result<(StatusCode, Json<Value>), StatusCode> {
    if *DEBUG_MODE {
        println!("--> [STORAGE] REBUILD URL INDEX");
    }
    let user = require_user_sub(&headers)?;
    let root = ensure_user_store(user.as_deref())?;
    let nodes = build_tree_recursive(&root, "");
    let secrets = flatten_secret_paths(&nodes);
    let mut entries = Vec::new();

    // Parallel decrypt with bounded concurrency (serial GPG over hundreds of secrets is very slow).
    let concurrency = 8usize;
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut set = tokio::task::JoinSet::new();

    for (path, title) in secrets {
        let root = root.clone();
        let user = user.clone();
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await.ok();
            let content = decrypt_metadata(&root, &path, user.as_deref()).await;
            (path, title, content)
        });
    }

    while let Some(joined) = set.join_next().await {
        let Ok((path, title, content)) = joined else { continue };
        let Some(content) = content else { continue };
        let (username, url) = parse_pass_metadata(&content);
        entries.push(crate::url_index::IndexEntry {
            path,
            title,
            username,
            url,
        });
    }

    let count = entries.len();
    crate::url_index::replace_all_in(&root, entries);
    Ok((StatusCode::OK, Json(json!({ "status": "ok", "entries": count }))))
}

/// On unlock, rebuild only when the plaintext index is missing/empty.
async fn refresh_url_index_after_unlock(headers: HeaderMap) {
    let Ok(user) = require_user_sub(&headers) else { return };
    let Ok(root) = ensure_user_store(user.as_deref()) else { return };
    if !crate::url_index::all_entries_in(&root).is_empty() {
        if *DEBUG_MODE {
            println!("--> [STORAGE] URL index already present — skip rebuild on unlock");
        }
        return;
    }
    let _ = rebuild_url_index(headers).await;
}

pub async fn decrypt_secret(
    headers: HeaderMap,
    Json(req): Json<ActionRequest>,
) -> Result<(StatusCode, Json<Value>), StatusCode> {
    let user = require_user_sub(&headers)?;
    let root = ensure_user_store(user.as_deref())?;
    if let Err(e) = validate_path(&req.path) {
        return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": e}))));
    }

    let file_path = format!("{}/{}.gpg", root, req.path);
    let encrypted_bytes = fs::read(file_path).unwrap_or_else(|_| vec![]);
    let encrypted_content = general_purpose::STANDARD.encode(&encrypted_bytes);
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();

    let client = reqwest::Client::new();
    match client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", shared_secret)
        .json(&bunker_task("decrypt", encrypted_content, user.as_deref()))
        .send().await
    {
        Ok(res) if res.status().is_success() => {
            let data: Value = res.json().await.unwrap_or(json!({"result": "Data error"}));
            let mut decrypted = data["result"].as_str().unwrap_or("").to_string();

            if !req.reveal.unwrap_or(false) {
                if let Some(first_line_end) = decrypted.find('\n') {
                    decrypted.replace_range(..first_line_end, "__TALOS_HIDDEN_SECRET__");
                } else {
                    decrypted = "__TALOS_HIDDEN_SECRET__".to_string();
                }
            }
            Ok((StatusCode::OK, Json(json!(decrypted))))
        },
        _ => Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!("Error: Bunker unavailable or decryption failed"))))
    }
}

pub async fn encrypt_and_save(headers: HeaderMap, Json(req): Json<ActionRequest>) -> Result<(StatusCode, Json<Value>), StatusCode> {
    let user = require_user_sub(&headers)?;
    let root = ensure_user_store(user.as_deref())?;
    log_audit_event("storage_save", "started", &format!("saving to path: {}", req.path));
    
    if *DEBUG_MODE { println!("--> [STORAGE] SAVE request for: {}", req.path); }

    // Validate path to prevent traversal attacks
    if let Err(e) = validate_path(&req.path) {
        log_audit_event("storage_save", "failed", &format!("path validation failed: {}", e));
        return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": e}))));
    }

    if let Some(ref original_path) = req.original_path {
        if let Err(e) = validate_path(original_path) {
            return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": e}))));
        }
    }

    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    
    let mut payload = req.content.unwrap_or_default();
    
    let client = reqwest::Client::new();
    
    if payload.starts_with("__TALOS_KEEP_SECRET__") {
        // Read the existing file the same way as decrypt_secret (binary → base64).
        // Using read_to_string corrupts binary .gpg payloads (e.g. migrated secrets)
        // and can leave the KEEP marker or garbage as the password.
        let keep_path = req.original_path.as_deref().unwrap_or(&req.path);
        let file_path = format!("{}/{}.gpg", root, keep_path);
        let encrypted_bytes = match fs::read(&file_path) {
            Ok(b) if !b.is_empty() => b,
            _ => {
                log_audit_event("storage_save", "failed", "keep-secret: could not read existing file");
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not preserve existing password"}))));
            }
        };
        let encrypted_content = general_purpose::STANDARD.encode(&encrypted_bytes);

        let decrypt_res = client.post(format!("{}/process", bunker_url))
            .header("X-Talos-Auth", &shared_secret)
            .json(&bunker_task("decrypt", encrypted_content, user.as_deref()))
            .send().await;

        let full_text = match decrypt_res {
            Ok(res) if res.status().is_success() => {
                let data: serde_json::Value = match res.json().await {
                    Ok(d) => d,
                    Err(_) => {
                        log_audit_event("storage_save", "failed", "keep-secret: invalid decrypt response");
                        return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not preserve existing password"}))));
                    }
                };
                let text = data["result"].as_str().unwrap_or("").to_string();
                if text.is_empty() || text.starts_with("ERROR") {
                    log_audit_event("storage_save", "failed", &format!("keep-secret: decrypt failed ({})", text));
                    return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not preserve existing password"}))));
                }
                if let Some(signature) = data["signature"].as_str() {
                    if !verify_signature(&text, signature) {
                        log_audit_event("storage_save", "failed", "signature verification failed during decrypt");
                        return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Signature verification failed"}))));
                    }
                }
                text
            }
            _ => {
                log_audit_event("storage_save", "failed", "keep-secret: bunker decrypt unavailable");
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not preserve existing password"}))));
            }
        };

        let old_pass = full_text.split('\n').next().unwrap_or("");
        // Replace only the leading marker (password may contain the marker string by chance).
        if let Some(rest) = payload.strip_prefix("__TALOS_KEEP_SECRET__") {
            payload = format!("{}{}", old_pass, rest);
        }
    }

    let (index_user, index_url) = parse_pass_metadata(&payload);
    let index_path = req.path.clone();
    let index_original = req.original_path.clone();

    let res_result = client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", &shared_secret)
        .json(&bunker_task("encrypt", payload, user.as_deref()))
        .send().await;

    match res_result {
        Ok(res) if res.status().is_success() => {
            let data: serde_json::Value = match res.json().await {
                Ok(d) => d,
                Err(_) => json!({"result": ""}),
            };
            let encrypted = data["result"].as_str().unwrap_or("").to_string();
            
            // Verify signature if present
            if let Some(signature) = data["signature"].as_str() {
                if !verify_signature(&encrypted, signature) {
                    log_audit_event("storage_save", "failed", "signature verification failed during encrypt");
                    return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Signature verification failed"}))));
                }
            }
            
            let armored_gpg = encrypted;
            
            if armored_gpg.is_empty() {
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Encryption failed"}))));
            }

            let file_path = format!("{}/{}.gpg", root, req.path);
            if let Some(parent) = std::path::Path::new(&file_path).parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    println!("❌ [STORAGE] Error creating directory: {}", e);
                    return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not create directory"}))));
                }
            }
            if let Err(e) = fs::write(&file_path, armored_gpg) {
                println!("❌ [STORAGE] Error writing file: {}", e);
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not write secret to disk"}))));
            }
            
            let mut commit_msg = format!("Update secret: {}", req.path);

            if let Some(original_path) = &req.original_path {
                if &req.path != original_path {
                    // This is a move operation
                    let old_file_path = format!("{}/{}.gpg", root, original_path);
                    if fs::remove_file(old_file_path).is_ok() {
                        if *DEBUG_MODE { println!("--> [STORAGE] Removed old file for move: {}", original_path); }
                        commit_msg = format!("Move secret from {} to {}", original_path, req.path);
                    }
                }
            }

            if let Some(ref original_path) = index_original {
                if original_path != &index_path {
                    crate::url_index::remove_entry_in(&root, original_path);
                }
            }
            crate::url_index::upsert_entry_in(&root, &index_path, index_user, index_url);
            
            commit_changes(&root, &commit_msg);
            Ok((StatusCode::OK, Json(json!({"status": "OK"}))))
        },
        _ => Ok((StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "Bunker unavailable"}))))
    }
}

pub async fn delete_entry(headers: HeaderMap, Json(req): Json<ActionRequest>) -> Result<(StatusCode, Json<Value>), StatusCode> {
    let user = require_user_sub(&headers)?;
    let root = ensure_user_store(user.as_deref())?;
    log_audit_event("storage_delete", "started", &format!("deleting path: {}", req.path));
    
    if *DEBUG_MODE { println!("--> [STORAGE] DELETE request for: {}", req.path); }

    // Validate path to prevent traversal attacks
    if let Err(e) = validate_path(&req.path) {
        log_audit_event("storage_delete", "failed", &format!("path validation failed: {}", e));
        return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": e}))));
    }

    let store_path = root.as_str();
    let path_as_dir = StdPath::new(store_path).join(&req.path);
    let path_as_file = StdPath::new(store_path).join(format!("{}.gpg", req.path));

    if path_as_file.is_file() {
        // Attempt to delete it as a file
        if fs::remove_file(&path_as_file).is_ok() {
            crate::url_index::remove_entry_in(&root, &req.path);
            commit_changes(&root, &format!("Delete secret: {}", req.path));
            Ok((StatusCode::OK, Json(json!({"status": "OK"}))))
        } else {
            Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not delete file"}))))
        }
    } else if path_as_dir.is_dir() {
        // Attempt to delete it as a directory
        match fs::read_dir(&path_as_dir) {
            Ok(dir) => {
                // Check if the directory contains anything other than .gitkeep
                let non_gitkeep_entries = dir.filter_map(Result::ok)
                    .filter(|e| e.file_name() != ".gitkeep")
                    .count();

                if non_gitkeep_entries == 0 {
                    if fs::remove_dir_all(&path_as_dir).is_ok() {
                        crate::url_index::remove_entry_in(&root, &req.path);
                        commit_changes(&root, &format!("Delete category: {}", req.path));
                        Ok((StatusCode::OK, Json(json!({"status": "OK"}))))
                    } else {
                        Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not delete directory"}))))
                    }
                } else {
                    Ok((StatusCode::CONFLICT, Json(json!({"error": "Category is not empty. Please remove all secrets and sub-categories first."}))))
                }
            }
            Err(_) => Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not read directory contents"}))))
        }
    } else {
        Ok((StatusCode::NOT_FOUND, Json(json!({"error": "Entry not found"}))))
    }
}

pub async fn create_category(headers: HeaderMap, Json(req): Json<ActionRequest>) -> Result<(StatusCode, Json<Value>), StatusCode> {
    let user = require_user_sub(&headers)?;
    let root = ensure_user_store(user.as_deref())?;
    if *DEBUG_MODE { println!("--> [STORAGE] CREATE CATEGORY request for: {}", req.path); }

    // Validate path to prevent traversal attacks
    if let Err(e) = validate_path(&req.path) {
        return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": e}))));
    }

    let dir_path = format!("{}/{}", root, req.path);
    
    if let Err(_) = fs::create_dir_all(&dir_path) {
        return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not create directory"}))));
    }

    let _ = fs::write(format!("{}/.gitkeep", dir_path), "");
    commit_changes(&root, &format!("Add category: {}", req.path));
    Ok((StatusCode::OK, Json(json!({"status": "OK"}))))
}

/// Rename/move a category directory. `original_path` is the current folder, `path` is the new one.
pub async fn rename_category(headers: HeaderMap, Json(req): Json<ActionRequest>) -> Result<(StatusCode, Json<Value>), StatusCode> {
    let user = require_user_sub(&headers)?;
    let root = ensure_user_store(user.as_deref())?;
    let Some(original_path) = req.original_path.as_deref() else {
        return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": "original_path required"}))));
    };

    if let Err(e) = validate_path(&req.path) {
        return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": e}))));
    }
    if let Err(e) = validate_path(original_path) {
        return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": e}))));
    }
    if req.path == original_path {
        return Ok((StatusCode::OK, Json(json!({"status": "OK"}))));
    }
    if req.path.is_empty() || original_path.is_empty() {
        return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": "Path required"}))));
    }

    let old_dir = format!("{}/{}", root, original_path);
    let new_dir = format!("{}/{}", root, req.path);
    let old_path = StdPath::new(&old_dir);
    let new_path = StdPath::new(&new_dir);

    if !old_path.is_dir() {
        return Ok((StatusCode::NOT_FOUND, Json(json!({"error": "Category not found"}))));
    }
    if new_path.exists() {
        return Ok((StatusCode::CONFLICT, Json(json!({"error": "Target category already exists"}))));
    }
    // Do not move a folder into one of its own descendants.
    let into_self = format!("{}/", original_path);
    if req.path.starts_with(&into_self) {
        return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": "Cannot move a category into itself"}))));
    }

    if let Some(parent) = new_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            println!("❌ [STORAGE] Error creating parent for rename: {}", e);
            return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not create parent directory"}))));
        }
    }

    if let Err(e) = fs::rename(&old_dir, &new_dir) {
        println!("❌ [STORAGE] Error renaming category: {}", e);
        return Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Could not rename category"}))));
    }

    crate::url_index::rename_prefix_in(&root, original_path, &req.path);
    commit_changes(&root, &format!("Rename category from {} to {}", original_path, req.path));
    Ok((StatusCode::OK, Json(json!({"status": "OK"}))))
}

pub async fn download_backup(headers: HeaderMap) -> Result<impl IntoResponse, StatusCode> {
    let user = require_user_sub(&headers)?;
    let store_path_owned = ensure_user_store(user.as_deref())?;
    let store_path = store_path_owned.as_str();

    if *DEBUG_MODE { println!("--> [STORAGE] BACKUP request initiated"); }
    
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
    
    // Calculate checksum after zip is complete
    let checksum = format!("{:x}", Sha256::digest(&buf));

    log_audit_event("storage_backup", "success", &format!("backup created with checksum: {}", checksum));

    Ok((
        [
            (header::CONTENT_TYPE, "application/zip"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"talos_backup.zip\""),
        ],
        buf,
    ))
}

pub async fn restore_backup(headers: HeaderMap, mut multipart: Multipart) -> Result<(StatusCode, Json<Value>), StatusCode> {
    let user = require_user_sub(&headers)?;
    let root = ensure_user_store(user.as_deref())?;
    log_audit_event("storage_restore", "started", "restoring from backup");
    
    if *DEBUG_MODE { println!("--> [STORAGE] RESTORE request initiated"); }
    
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if field.name() == Some("backup") {
            let data = field.bytes().await.unwrap_or_default();
            
            // Verify integrity checksum
            let expected_checksum = format!("{:x}", Sha256::digest(&data));
            
            let reader = Cursor::new(data);
            let mut archive = match zip::ZipArchive::new(reader) {
                Ok(a) => a,
                Err(_) => {
                    log_audit_event("storage_restore", "failed", "invalid zip file");
                    return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid zip file"}))));
                },
            };
            
            // Check for checksum file and verify
            let mut found_checksum = false;
            for i in 0..archive.len() {
                if let Ok(mut file) = archive.by_index(i) {
                    if file.name() == "SHA256_CHECKSUM.txt" {
                        if let Ok(checksum_content) = std::io::read_to_string(&mut file) {
                            found_checksum = true;
                            // Extract just the checksum (remove any whitespace)
                            let stored_checksum = checksum_content.trim();
                            if stored_checksum != expected_checksum {
                                log_audit_event("storage_restore", "failed", "integrity check failed - checksum mismatch");
                                return Ok((StatusCode::BAD_REQUEST, Json(json!({"error": "Integrity verification failed"}))));
                            }
                        }
                    }
                }
            }
            
            if !found_checksum {
                log_audit_event("storage_restore", "warning", "no checksum file found, proceeding without verification");
            } else {
                log_audit_event("storage_restore", "success", "integrity verification passed");
            }

            let store_path = root.as_str();
            
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
            
            commit_changes(&root, "Restored from backup");
            return Ok((StatusCode::OK, Json(json!({"status": "Restored successfully"}))));
        }
    }
    Ok((StatusCode::BAD_REQUEST, Json(json!({"error": "No backup file provided"}))))
}

#[derive(serde::Deserialize)]
pub struct InitializeRequest {
    pub key: String,
    #[serde(default)]
    pub user_sub: Option<String>,
}

pub async fn initialize_bunker(headers: HeaderMap, Json(req): Json<InitializeRequest>) -> impl IntoResponse {
    if *DEBUG_MODE { println!("--> [STORAGE] INITIALIZE request received"); }
    let user_sub = match vault_user_from_headers(&headers) {
        Ok(u) => u,
        Err(status) => return (status, Json(json!({"error": "Unauthorized"}))).into_response(),
    };
    let user_ref = user_sub.as_deref();
    if let Some(sub) = user_ref {
        let _ = ensure_user_store(Some(sub));
    }
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    let client = reqwest::Client::new();

    // Check if the bunker is already initialized for this user
    let check_res: serde_json::Value = client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", &shared_secret)
        .json(&bunker_task("check", String::new(), user_ref))
        .send().await.unwrap().json().await.unwrap();
    
    if check_res["result"].as_str() != Some("UNINITIALIZED") {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "System already initialized"}))).into_response();
    }

    // Send Initialize Command
    let init_res = client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", &shared_secret)
        .json(&bunker_task("initialize", req.key, user_ref))
        .send().await;

    match init_res {
        Ok(res) if res.status().is_success() => {
            let data: serde_json::Value = res.json().await.unwrap_or_default();
            if data["result"].as_str() == Some("INITIALIZED") {
                (StatusCode::OK, Json(json!({"status": "initialized"}))).into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Bunker initialization failed"}))).into_response()
            }
        },
        _ => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "Bunker unreachable or init failed"}))).into_response()
    }
}

#[derive(serde::Deserialize)]
pub struct ImportRequest {
    pub key: String,
    pub passphrase: String,
    #[serde(default)]
    pub user_sub: Option<String>,
}

pub async fn import_bunker_key(headers: HeaderMap, Json(req): Json<ImportRequest>) -> impl IntoResponse {
    let user_sub = match vault_user_from_headers(&headers) {
        Ok(u) => u,
        Err(status) => return (status, Json(json!({"error": "Unauthorized"}))).into_response(),
    };
    let user_ref = user_sub.as_deref();
    if let Some(sub) = user_ref {
        let _ = ensure_user_store(Some(sub));
    }

    if *DEBUG_MODE { println!("--> [STORAGE] IMPORT KEY request received"); }
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    let client = reqwest::Client::new();

    // Send Import Command to Bunker
    // We send the private key block and the passphrase to unlock/verify it
    let import_res = client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", &shared_secret)
        .json(&BunkerTask {
            payload: req.key,
            mode: "import".to_string(),
            signature: None,
            user_sub: user_sub.clone(),
            passphrase: Some(req.passphrase),
        })
        .send().await;

    match import_res {
        Ok(res) if res.status().is_success() => {
            (StatusCode::OK, Json(json!({"status": "imported"}))).into_response()
        },
        _ => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Import failed"}))).into_response()
    }
}

// Track which vaults already exported their key this process lifetime.
static KEY_DOWNLOADED: once_cell::sync::Lazy<std::sync::Mutex<std::collections::HashSet<String>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

pub async fn backup_bunker_key(headers: HeaderMap) -> impl IntoResponse {
    let user_sub = match vault_user_from_headers(&headers) {
        Ok(u) => u,
        Err(status) => return (status, Json(json!({"error": "Unauthorized"}))).into_response(),
    };
    let download_key = user_sub
        .clone()
        .unwrap_or_else(|| "__legacy__".to_string());

    {
        let downloaded = KEY_DOWNLOADED.lock().unwrap();
        if downloaded.contains(&download_key) {
            return (StatusCode::GONE, Json(json!({"error": "Key already downloaded. Access revoked."}))).into_response();
        }
    }

    if *DEBUG_MODE { println!("--> [STORAGE] BACKUP KEY request"); }
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    let client = reqwest::Client::new();

    // Request export from Bunker (scoped to verified user when MULTIUSER)
    let res = client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", &shared_secret)
        .json(&bunker_task("export_key", String::new(), user_sub.as_deref()))
        .send().await;

    match res {
        Ok(response) if response.status().is_success() => {
            let data: Value = response.json().await.unwrap_or_default();
            let key_content = data["result"].as_str().unwrap_or("").to_string();

            KEY_DOWNLOADED.lock().unwrap().insert(download_key);

            (StatusCode::OK, key_content).into_response()
        },
        _ => (StatusCode::BAD_GATEWAY, Json(json!({"error": "Failed to export key"}))).into_response()
    }
}

#[derive(serde::Deserialize)]
pub struct UnlockRequest {
    pub key: String,
    #[serde(default)]
    pub user_sub: Option<String>,
}

pub async fn unlock_bunker(headers: HeaderMap, Json(req): Json<UnlockRequest>) -> impl IntoResponse {
    let user_sub = match vault_user_from_headers(&headers) {
        Ok(u) => u,
        Err(status) => return (status, Json(json!({"error": "Unauthorized"}))),
    };
    let user_ref = user_sub.as_deref();
    if let Some(sub) = user_ref {
        let _ = ensure_user_store(Some(sub));
    }
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    let client = reqwest::Client::new();

    // 1. Send Unlock Command (Inject Key into Bunker RAM)
    let unlock_res = client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", &shared_secret)
        .json(&bunker_task("unlock", req.key.clone(), user_ref))
        .send().await;

    let unlock_body: Value = match unlock_res {
        Ok(response) if response.status().is_success() => response.json().await.unwrap_or_default(),
        _ => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "Bunker unreachable"}))),
    };
    let unlock_result = unlock_body["result"].as_str().unwrap_or("");
    if unlock_result.starts_with("ERROR") || unlock_result == "UNINITIALIZED" {
        return (StatusCode::UNAUTHORIZED, Json(json!({
            "error": if unlock_result == "UNINITIALIZED" || unlock_result.contains("GPG") {
                "Vault not initialized — create a vault passphrase first"
            } else {
                "Vault unlock failed"
            }
        })));
    }
    if unlock_result != "VAULT_UNSEALED" {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "Vault unlock failed"})));
    }

    // 2. Verify Key Validity (The "True Master Key" Check)
    // We attempt to encrypt and then decrypt a canary string. 
    // If the key is wrong, GPG will fail at the encryption or decryption step.
    let test_payload = "TALOS_VERIFY_SEQ";
    
    // A. Encrypt
    let enc_res = client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", &shared_secret)
        .json(&bunker_task("encrypt", test_payload.to_string(), user_ref))
        .send().await;

    let encrypted = match enc_res {
        Ok(res) if res.status().is_success() => {
            let data: serde_json::Value = res.json().await.unwrap_or_default();
            let result = data["result"].as_str().unwrap_or("").to_string();
            result
        },
        _ => {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "Key rejected (Encryption failed)"})));
        }
    };

    // B. Decrypt
    let dec_res = client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", &shared_secret)
        .json(&bunker_task("decrypt", encrypted, user_ref))
        .send().await;

    let decrypted = match dec_res {
        Ok(res) if res.status().is_success() => {
            let data: serde_json::Value = res.json().await.unwrap_or_default();
            data["result"].as_str().unwrap_or("").to_string()
        },
        _ => return (StatusCode::UNAUTHORIZED, Json(json!({"error": "Key rejected (Decryption failed)"}))),
    };

    if decrypted.trim() == test_payload {
        // Refresh URL index while bunker is unsealed so match works without auth later.
        // Skip when an index already exists (rebuild of hundreds of secrets is slow).
        refresh_url_index_after_unlock(headers).await;
        (StatusCode::OK, Json(json!({"status": "unlocked"})))
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid Master Key"})))
    }
}

fn commit_changes(store_path: &str, msg: &str) {
    if CONFIG.backend.r#type == "git" {
        // Never push the plaintext URL index to the remote.
        crate::url_index::ensure_index_ignored_for_commit(store_path);
        let _ = std::process::Command::new("git").args(["-C", store_path, "add", "-A"]).status();
        let _ = std::process::Command::new("git")
            .args(["-C", store_path, "reset", "HEAD", "--", ".talos-url-index.json"])
            .status();
        let _ = std::process::Command::new("git").args(["-C", store_path, "commit", "-m", msg]).status();
        
        if *DEBUG_MODE { println!("--> [STORAGE] Pushing changes to remote git..."); }
        let _ = std::process::Command::new("git").args(["-C", store_path, "push", "origin", "HEAD"]).status();
    } else {
        if *DEBUG_MODE { println!("--> [STORAGE] Local change recorded (No Git configured). Backup available at: {}", store_path); }
    }
}

// Health check handler to verify connectivity with the Bunker

#[derive(serde::Deserialize)]
pub struct OperatorUnsealRequest {
    pub key: String,
}

pub async fn operator_unseal(Json(req): Json<OperatorUnsealRequest>) -> impl IntoResponse {
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    let client = reqwest::Client::new();
    let res = client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", &shared_secret)
        .json(&bunker_task("operator_unseal", req.key, None))
        .send().await;
    match res {
        Ok(r) if r.status().is_success() => {
            let data: Value = r.json().await.unwrap_or_default();
            if data["result"].as_str() == Some("OPERATOR_UNSEALED") {
                (StatusCode::OK, Json(json!({"status": "operator_unsealed"})))
            } else {
                (StatusCode::BAD_REQUEST, Json(json!({"error": data["result"]})))
            }
        }
        _ => (StatusCode::BAD_GATEWAY, Json(json!({"error": "Bunker unreachable"})))
    }
}

pub async fn operator_status() -> impl IntoResponse {
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    let client = reqwest::Client::new();
    let res = client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", &shared_secret)
        .json(&bunker_task("operator_status", String::new(), None))
        .send().await;
    match res {
        Ok(r) if r.status().is_success() => {
            let data: Value = r.json().await.unwrap_or_default();
            (StatusCode::OK, Json(json!({"status": data["result"]})))
        }
        _ => (StatusCode::BAD_GATEWAY, Json(json!({"error": "Bunker unreachable"})))
    }
}

pub async fn unlock_wrapped(headers: HeaderMap) -> Result<impl IntoResponse, StatusCode> {
    let user = require_user_sub(&headers)?;
    let sub = user.ok_or(StatusCode::UNAUTHORIZED)?;
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();
    let client = reqwest::Client::new();
    let res = client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", &shared_secret)
        .json(&bunker_task("unlock_wrapped", String::new(), Some(&sub)))
        .send().await;
    match res {
        Ok(r) if r.status().is_success() => {
            let data: Value = r.json().await.unwrap_or_default();
            if data["result"].as_str() == Some("VAULT_UNSEALED") {
                refresh_url_index_after_unlock(headers.clone()).await;
                Ok((StatusCode::OK, Json(json!({"status": "unlocked"}))))
            } else {
                Ok((StatusCode::UNAUTHORIZED, Json(json!({"error": data["result"]}))))
            }
        }
        _ => Ok((StatusCode::BAD_GATEWAY, Json(json!({"error": "Bunker unreachable"}))))
    }
}

pub async fn storage_health_check(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    let user = extract_user_sub(&headers)?;
    let client = reqwest::Client::new();
    let bunker_url = env::var("BUNKER_URL").unwrap_or_else(|_| "http://talos-bunker:5000".to_string());
    let shared_secret = env::var("SHARED_SECRET").unwrap_or_default();

    // Storage communicates with Bunker over the isolated private network.
    // When identity headers are present, check that user's vault (not legacy).
    let bunker_res = client.post(format!("{}/process", bunker_url))
        .header("X-Talos-Auth", &shared_secret)
        .json(&bunker_task("check", String::new(), user.as_deref()))
        .send().await;

    let bunker_status = match bunker_res {
        Ok(res) if res.status().is_success() => {
            let data: serde_json::Value = res.json().await.unwrap_or(json!({"result": "ERROR"}));
            data["result"].as_str().unwrap_or("ERROR").to_string()
        },
        _ => "OFFLINE".to_string(),
    };

    Ok(Json(json!({
        "storage": true, // Storage is reachable if this code executes
        "bunker": bunker_status,
        "user_scoped": user.is_some(),
    })))
}