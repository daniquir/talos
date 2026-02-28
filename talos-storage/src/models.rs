use serde::{Deserialize, Serialize};

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