//! OIDC (Keycloak) helpers: authorize URL, code exchange, JWT claims.

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct OidcConfig {
    pub enabled: bool,
    pub issuer: String,
    pub issuer_internal: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    /// Accepted `aud` values for id_tokens (web + extension clients).
    pub audiences: Vec<String>,
}

impl OidcConfig {
    pub fn from_env() -> Self {
        let multiuser = matches!(
            env::var("MULTIUSER").unwrap_or_else(|_| "true".into()).to_lowercase().as_str(),
            "1" | "true" | "yes"
        );
        let issuer = env::var("OIDC_ISSUER").unwrap_or_default();
        let client_id = env::var("OIDC_CLIENT_ID").unwrap_or_else(|_| "talos-web".into());
        // Web confidential client + public extension client both mint valid id_tokens.
        let audiences = env::var("OIDC_AUDIENCES")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                let mut v = vec![client_id.clone()];
                if client_id != "talos-extension" {
                    v.push("talos-extension".into());
                }
                v
            });
        Self {
            enabled: multiuser && !issuer.is_empty(),
            issuer,
            issuer_internal: env::var("OIDC_ISSUER_INTERNAL")
                .unwrap_or_else(|_| env::var("OIDC_ISSUER").unwrap_or_default()),
            client_id,
            client_secret: env::var("OIDC_CLIENT_SECRET").unwrap_or_default(),
            redirect_uri: env::var("OIDC_REDIRECT_URI")
                .unwrap_or_else(|_| "http://localhost:3000/api/auth/oidc/callback".into()),
            audiences,
        }
    }

    pub fn authorize_url(&self, state: &str, code_challenge: &str) -> String {
        format!(
            "{}/protocol/openid-connect/auth?client_id={}&redirect_uri={}&response_type=code&scope=openid%20profile%20email&state={}&code_challenge={}&code_challenge_method=S256",
            self.issuer.trim_end_matches('/'),
            urlencoding::encode(&self.client_id),
            urlencoding::encode(&self.redirect_uri),
            urlencoding::encode(state),
            urlencoding::encode(code_challenge),
        )
    }

    pub fn token_url(&self) -> String {
        format!(
            "{}/protocol/openid-connect/token",
            self.issuer_internal.trim_end_matches('/')
        )
    }

    pub fn jwks_url(&self) -> String {
        format!(
            "{}/protocol/openid-connect/certs",
            self.issuer_internal.trim_end_matches('/')
        )
    }
}

pub fn custody_mode() -> String {
    env::var("TALOS_CUSTODY_MODE")
        .unwrap_or_else(|_| "strict".into())
        .to_lowercase()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IdClaims {
    pub sub: String,
    pub email: Option<String>,
    pub preferred_username: Option<String>,
    #[serde(default)]
    pub realm_access: Option<RealmAccess>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RealmAccess {
    #[serde(default)]
    pub roles: Vec<String>,
}

impl IdClaims {
    pub fn is_admin(&self) -> bool {
        self.realm_access
            .as_ref()
            .map(|r| r.roles.iter().any(|x| x == "talos-admin"))
            .unwrap_or(false)
    }
}

#[derive(Clone, Default)]
pub struct JwksCache {
    inner: Arc<Mutex<Option<(Instant, Value)>>>,
}

impl JwksCache {
    pub async fn get_keys(&self, cfg: &OidcConfig) -> Result<Value, String> {
        {
            let guard = self.inner.lock().map_err(|e| e.to_string())?;
            if let Some((at, ref v)) = *guard {
                if at.elapsed() < Duration::from_secs(3600) {
                    return Ok(v.clone());
                }
            }
        }
        let client = reqwest::Client::new();
        let res = client
            .get(cfg.jwks_url())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let jwks: Value = res.json().await.map_err(|e| e.to_string())?;
        let mut guard = self.inner.lock().map_err(|e| e.to_string())?;
        *guard = Some((Instant::now(), jwks.clone()));
        Ok(jwks)
    }
}

pub async fn exchange_code(
    cfg: &OidcConfig,
    code: &str,
    code_verifier: &str,
) -> Result<(String, IdClaims), String> {
    let client = reqwest::Client::new();
    let mut form = HashMap::new();
    form.insert("grant_type", "authorization_code");
    form.insert("code", code);
    form.insert("redirect_uri", cfg.redirect_uri.as_str());
    form.insert("client_id", cfg.client_id.as_str());
    form.insert("client_secret", cfg.client_secret.as_str());
    form.insert("code_verifier", code_verifier);

    let res = client
        .post(cfg.token_url())
        .form(&form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(format!("token exchange failed: {}", body));
    }
    let data: Value = res.json().await.map_err(|e| e.to_string())?;
    let id_token = data["id_token"]
        .as_str()
        .ok_or("missing id_token")?
        .to_string();
    Ok((id_token, IdClaims {
        sub: String::new(),
        email: None,
        preferred_username: None,
        realm_access: None,
    }))
}

pub async fn validate_id_token(
    cfg: &OidcConfig,
    jwks: &JwksCache,
    id_token: &str,
) -> Result<IdClaims, String> {
    let header = decode_header(id_token).map_err(|e| e.to_string())?;
    let kid = header.kid.ok_or("missing kid")?;
    let keys = jwks.get_keys(cfg).await?;
    let keys_arr = keys["keys"].as_array().ok_or("bad jwks")?;
    let jwk = keys_arr
        .iter()
        .find(|k| k["kid"].as_str() == Some(kid.as_str()))
        .ok_or("kid not found")?;
    let n = jwk["n"].as_str().ok_or("missing n")?;
    let e = jwk["e"].as_str().ok_or("missing e")?;
    let key = DecodingKey::from_rsa_components(n, e).map_err(|e| e.to_string())?;

    let mut validation = Validation::new(Algorithm::RS256);
    let aud_refs: Vec<&str> = cfg.audiences.iter().map(|s| s.as_str()).collect();
    validation.set_audience(&aud_refs);
    // Issuer in token is public issuer URL
    validation.set_issuer(&[cfg.issuer.trim_end_matches('/')]);
    validation.validate_exp = true;

    let token = decode::<IdClaims>(id_token, &key, &validation).map_err(|e| e.to_string())?;
    Ok(token.claims)
}

pub fn pkce_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

pub fn random_string(len: usize) -> String {
    use rand::RngCore;
    let mut bytes = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}
