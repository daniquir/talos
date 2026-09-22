//! Plaintext URL/username index for host matching without unsealing the bunker.
//! Passwords are never stored here — only path, title, username, and URL.
//! Scoped to a password-store root (per-user when MULTIUSER=true).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use crate::config::STORE_PATH;

const INDEX_FILE: &str = ".talos-url-index.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub path: String,
    pub title: String,
    pub username: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct UrlIndex {
    entries: Vec<IndexEntry>,
}

fn index_path_in(store_root: &str) -> String {
    format!("{}/{}", store_root, INDEX_FILE)
}

fn load_index_in(store_root: &str) -> UrlIndex {
    let path = index_path_in(store_root);
    match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => UrlIndex::default(),
    }
}

fn save_index_in(store_root: &str, index: &UrlIndex) {
    let path = index_path_in(store_root);
    if let Some(parent) = Path::new(&path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(index) {
        let _ = fs::write(&path, raw);
    }
    ensure_index_gitignored(store_root);
}

/// Keep the plaintext URL index out of git commits / remotes.
fn ensure_index_gitignored(store_path: &str) {
    let gi_path = format!("{}/.gitignore", store_path);
    let needle = INDEX_FILE;
    let existing = fs::read_to_string(&gi_path).unwrap_or_default();
    if !existing.lines().any(|l| l.trim() == needle) {
        let mut next = existing;
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(needle);
        next.push('\n');
        let _ = fs::write(&gi_path, next);
    }
    let _ = std::process::Command::new("git")
        .args(["-C", store_path, "rm", "--cached", "-f", "--ignore-unmatch", needle])
        .status();
}

pub fn ensure_index_ignored_for_commit(store_path: &str) {
    ensure_index_gitignored(store_path);
}

pub fn upsert_entry_in(store_root: &str, path: &str, username: Option<String>, url: Option<String>) {
    let title = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string();
    let mut index = load_index_in(store_root);
    if let Some(existing) = index.entries.iter_mut().find(|e| e.path == path) {
        existing.title = title;
        existing.username = username;
        existing.url = url;
    } else {
        index.entries.push(IndexEntry {
            path: path.to_string(),
            title,
            username,
            url,
        });
    }
    save_index_in(store_root, &index);
}

pub fn remove_entry_in(store_root: &str, path: &str) {
    let mut index = load_index_in(store_root);
    let before = index.entries.len();
    index.entries.retain(|e| e.path != path && !e.path.starts_with(&format!("{}/", path)));
    if index.entries.len() != before {
        save_index_in(store_root, &index);
    }
}

pub fn rename_entry_in(store_root: &str, old_path: &str, new_path: &str) {
    let mut index = load_index_in(store_root);
    if let Some(entry) = index.entries.iter_mut().find(|e| e.path == old_path) {
        entry.path = new_path.to_string();
        entry.title = new_path
            .rsplit('/')
            .next()
            .unwrap_or(new_path)
            .to_string();
        save_index_in(store_root, &index);
    }
}

pub fn replace_all_in(store_root: &str, entries: Vec<IndexEntry>) {
    save_index_in(store_root, &UrlIndex { entries });
}

pub fn all_entries_in(store_root: &str) -> Vec<IndexEntry> {
    load_index_in(store_root).entries
}

// --- Legacy wrappers (single-tenant root) ---

pub fn upsert_entry(path: &str, username: Option<String>, url: Option<String>) {
    upsert_entry_in(STORE_PATH.as_str(), path, username, url);
}
pub fn remove_entry(path: &str) {
    remove_entry_in(STORE_PATH.as_str(), path);
}
pub fn rename_entry(old_path: &str, new_path: &str) {
    rename_entry_in(STORE_PATH.as_str(), old_path, new_path);
}
pub fn replace_all(entries: Vec<IndexEntry>) {
    replace_all_in(STORE_PATH.as_str(), entries);
}
pub fn all_entries() -> Vec<IndexEntry> {
    all_entries_in(STORE_PATH.as_str())
}
