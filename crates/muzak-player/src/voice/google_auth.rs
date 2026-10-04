//! Short-lived Google access tokens from a service account key file, for Vertex AI.
//!
//! Signs a JWT with the account's private key and swaps it at Google's token endpoint
//! (the OAuth 2.0 JWT bearer flow). Tokens last an hour and are reused until near expiry.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

const SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

#[derive(Deserialize)]
struct KeyFile {
    client_email: String,
    private_key: String,
    project_id: String,
    #[serde(default)]
    token_uri: Option<String>,
}

pub struct ServiceAccount {
    email: String,
    key: RsaKeyPair,
    token_uri: String,
    pub project_id: String,
    cached: Mutex<Option<(String, Instant)>>,
}

/// The DER bytes inside a PEM block.
fn pem_to_der(pem: &str) -> anyhow::Result<Vec<u8>> {
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<Vec<_>>()
        .concat();
    Ok(STANDARD.decode(body.trim())?)
}

impl ServiceAccount {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let file: KeyFile = serde_json::from_slice(
            &std::fs::read(path).with_context(|| format!("reading {}", path.display()))?,
        )
        .with_context(|| format!("{} isn't a service account key file", path.display()))?;
        let key = RsaKeyPair::from_pkcs8(&pem_to_der(&file.private_key)?)
            .map_err(|e| anyhow!("the service account's private key is unreadable: {e}"))?;
        Ok(Self {
            email: file.client_email,
            key,
            token_uri: file.token_uri.unwrap_or_else(|| DEFAULT_TOKEN_URI.into()),
            project_id: file.project_id,
            cached: Mutex::new(None),
        })
    }

    /// The signed assertion asking for a token.
    fn assertion(&self, now: u64) -> anyhow::Result<String> {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let claims = URL_SAFE_NO_PAD.encode(
            json!({
                "iss": self.email,
                "scope": SCOPE,
                "aud": self.token_uri,
                "iat": now,
                "exp": now + 3600,
            })
            .to_string(),
        );
        let message = format!("{header}.{claims}");
        let mut signature = vec![0; self.key.public().modulus_len()];
        self.key
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                message.as_bytes(),
                &mut signature,
            )
            .map_err(|_| anyhow!("signing the token request failed"))?;
        Ok(format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature)))
    }

    /// A valid access token, fetched again when the old one is about to expire.
    pub async fn token(&self, http: &reqwest::Client) -> anyhow::Result<String> {
        let mut cached = self.cached.lock().await;
        if let Some((token, until)) = cached.as_ref()
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        #[derive(Deserialize)]
        struct Reply {
            access_token: String,
            expires_in: u64,
        }
        let response = http
            .post(&self.token_uri)
            .header("content-type", "application/x-www-form-urlencoded")
            // The assertion is base64url and dots, which need no escaping.
            .body(format!(
                "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={}",
                self.assertion(now)?
            ))
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Google refused the service account ({status}): {body}"
            ));
        }
        let reply: Reply = response.json().await?;
        // Renew a few minutes early.
        let until = Instant::now() + Duration::from_secs(reply.expires_in.saturating_sub(300));
        *cached = Some((reply.access_token.clone(), until));
        Ok(reply.access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With a real key file: `VERTEX_KEY_FILE=… cargo test -- --ignored live_service_account`.
    #[tokio::test]
    #[ignore]
    async fn live_service_account() {
        let Ok(path) = std::env::var("VERTEX_KEY_FILE") else {
            return;
        };
        let account = ServiceAccount::load(Path::new(&path)).unwrap();
        let http = reqwest::Client::new();
        let first = account.token(&http).await.unwrap();
        assert!(first.len() > 100);
        assert_eq!(
            account.token(&http).await.unwrap(),
            first,
            "the token is reused"
        );
    }
}
