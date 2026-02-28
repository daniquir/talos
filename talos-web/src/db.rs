use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;

pub type DbPool = Pool<Sqlite>;

pub async fn init_db() -> DbPool {
    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    
    // Ensure SQLite creates the file if it doesn't exist by adding mode=rwc
    let url = if !db_url.contains("mode=rwc") {
        format!("{}?mode=rwc", db_url)
    } else {
        db_url.clone()
    };

    println!("📦 [DB] Connecting to database...");
    let pool = SqlitePoolOptions::new()
        .connect(&url)
        .await
        .expect("Failed to connect to database");

    // HARDENING: Restrict talos.db file permissions to 600 (Only owner can read/write)
    if let Some(path_str) = db_url.strip_prefix("sqlite:") {
        // Clean up extra URL options if any (e.g., ?mode=rwc)
        let clean_path = path_str.split('?').next().unwrap_or(path_str);
        if let Ok(metadata) = fs::metadata(clean_path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o600); // Read/Write only for owner
            if let Err(e) = fs::set_permissions(clean_path, perms) {
                println!("⚠️ [DB] Warning: Could not set secure permissions (0600) on DB: {}", e);
            } else {
                println!("🔒 [DB] Secure permissions (0600) applied to database file.");
            }
        }
    }

    // Create audit table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS audit_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            action TEXT NOT NULL,
            target TEXT NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            ip_address TEXT,
            user_agent TEXT,
            auth_method TEXT
        )"
    )
    .execute(&pool)
    .await
    .expect("Failed to initialize audit schema");

    pool
}