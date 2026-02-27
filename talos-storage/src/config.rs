use serde::Deserialize;
use once_cell::sync::Lazy;
use std::fs;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub backend: Backend,
}

#[derive(Deserialize, Debug)]
pub struct Backend {
    pub r#type: String, // "git" or "local"
    pub repository_url: Option<String>,
    pub ssh_key_path: Option<String>,
}

pub static CONFIG: Lazy<Config> = Lazy::new(|| {
    let config_path = "/app/config/storage.json";
    match fs::read_to_string(config_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            println!("⚠️ Failed to parse config: {}. Defaulting to LOCAL backend.", e);
            Config::default()
        }),
        Err(_) => {
            println!("⚠️ Config file not found at {}. Defaulting to LOCAL backend.", config_path);
            Config::default()
        }
    }
});

pub static DEBUG_MODE: Lazy<bool> = Lazy::new(|| {
    std::env::var("DEBUG").unwrap_or_default() == "true"
});

pub static STORE_PATH: Lazy<String> = Lazy::new(|| {
    std::env::var("PASSWORD_STORE_DIR")
        .unwrap_or_else(|_| "/home/talosuser/.password-store".to_string())
});

impl Default for Config {
    fn default() -> Self {
        Config {
            backend: Backend {
                r#type: "local".to_string(),
                repository_url: None,
                ssh_key_path: None,
            }
        }
    }
}