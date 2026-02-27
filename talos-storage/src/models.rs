use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct PassEntry { pub name: String, pub is_dir: bool }

#[derive(Deserialize)]
pub struct ActionRequest {
    pub path: String,
    pub content: Option<String>,
    pub original_path: Option<String>,
    pub reveal: Option<bool>,
}

#[derive(Serialize)]
pub struct BunkerTask {
    pub payload: String,
    pub mode: String,
}