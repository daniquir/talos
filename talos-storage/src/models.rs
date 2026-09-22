use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct ActionRequest {
    pub path: String,
    pub content: Option<String>,
    pub original_path: Option<String>,
    pub reveal: Option<bool>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct BunkerTask {
    pub payload: String,
    pub mode: String,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub user_sub: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
}

impl BunkerTask {
    pub fn with_user(mut self, user_sub: Option<&str>) -> Self {
        self.user_sub = user_sub.map(|s| s.to_string());
        self
    }
}
