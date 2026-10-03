//! Web API access tokens minted by the librespot session.

use std::time::Duration;

use librespot::core::session::Session;
use tokio::sync::watch;

use super::FetchError;
use super::web_api::{SCOPES, TokenSource};

pub struct SessionTokens {
    session: watch::Receiver<Option<Session>>,
}

impl SessionTokens {
    pub fn new(session: watch::Receiver<Option<Session>>) -> Self {
        Self { session }
    }
}

impl TokenSource for SessionTokens {
    async fn token(&self) -> Result<String, FetchError> {
        let mut rx = self.session.clone();
        let session = {
            let waited =
                tokio::time::timeout(Duration::from_secs(20), rx.wait_for(|s| s.is_some())).await;
            match waited {
                Ok(Ok(guard)) => guard.clone(),
                _ => None,
            }
        };
        let Some(session) = session else {
            return Err(FetchError::Offline);
        };
        session
            .token_provider()
            .get_token(SCOPES)
            .await
            .map(|t| t.access_token)
            .map_err(|e| {
                tracing::warn!("token request failed: {e}");
                FetchError::Offline
            })
    }
}
