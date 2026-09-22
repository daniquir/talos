//! Persisted UI settings (shared by web UI and browser extension).
//! Stored in SQLite alongside audit logs.

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use crate::state::AppState;

const KEY_LANG: &str = "lang";
const DEFAULT_LANG: &str = "auto";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSettings {
    /// `auto` | `en` | `es`
    pub lang: String,
}

fn normalize_lang(raw: &str) -> String {
    match raw.trim().to_lowercase().as_str() {
        "en" | "es" | "auto" => raw.trim().to_lowercase(),
        _ => DEFAULT_LANG.to_string(),
    }
}

pub async fn ensure_settings_schema(pool: &sqlx::SqlitePool) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ui_settings (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("Failed to initialize ui_settings schema");
}

async fn get_setting(pool: &sqlx::SqlitePool, key: &str) -> Option<String> {
    let row = sqlx::query("SELECT value FROM ui_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()??;
    row.try_get::<String, _>("value").ok()
}

async fn set_setting(pool: &sqlx::SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO ui_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn load_settings(pool: &sqlx::SqlitePool) -> UiSettings {
    let lang = get_setting(pool, KEY_LANG)
        .await
        .map(|v| normalize_lang(&v))
        .unwrap_or_else(|| DEFAULT_LANG.to_string());
    UiSettings { lang }
}

/// Public: both web and extension read the shared preference (no secrets).
pub async fn get_settings(State(state): State<AppState>) -> Json<UiSettings> {
    Json(load_settings(&state.pool).await)
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    pub lang: Option<String>,
}

/// Authenticated: update shared UI settings (Bearer or session).
pub async fn put_settings(
    State(state): State<AppState>,
    Json(body): Json<UpdateSettingsRequest>,
) -> Result<Json<UiSettings>, StatusCode> {
    if let Some(lang) = body.lang {
        let lang = normalize_lang(&lang);
        set_setting(&state.pool, KEY_LANG, &lang)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(Json(load_settings(&state.pool).await))
}
