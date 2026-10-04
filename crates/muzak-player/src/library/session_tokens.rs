//! Web API access tokens minted by the librespot session.

use std::sync::Mutex;
use std::time::Duration;

use librespot::core::session::Session;
use tokio::sync::watch;

use super::FetchError;
use super::web_api::{SCOPES, TokenSource};

const TOKEN_TIMEOUT: Duration = Duration::from_secs(20);

/// librespot caches a token for any request whose scopes are a subset of a cached token's scopes
/// and exposes no way to clear the cache. After a 401 we ask for the same scopes plus one extra
/// harmless scope, which misses the cache and returns a fresh token. Cycling the extra scope keeps
/// a second invalidation from hitting the token the first one cached.
const EXTRA_SCOPES: [&str; 3] = ["user-read-private", "user-read-email", "user-follow-read"];

#[derive(Default)]
struct Pinned {
    invalidations: usize,
    needs_fresh: bool,
    token: Option<String>,
}

pub struct SessionTokens {
    session: watch::Receiver<Option<Session>>,
    pinned: Mutex<Pinned>,
}

impl SessionTokens {
    pub fn new(session: watch::Receiver<Option<Session>>) -> Self {
        Self {
            session,
            pinned: Mutex::new(Pinned::default()),
        }
    }

    async fn fetch(&self) -> Result<String, FetchError> {
        let mut rx = self.session.clone();
        let session = match rx.wait_for(|s| s.is_some()).await {
            Ok(guard) => guard.clone(),
            // The player task dropped the sender: it gave up on the credentials.
            Err(_) => return Err(FetchError::Auth),
        };
        let Some(session) = session else {
            return Err(FetchError::Offline);
        };

        let (needs_fresh, extra) = {
            let pinned = self.pinned.lock().unwrap();
            if let Some(token) = &pinned.token {
                return Ok(token.clone());
            }
            (
                pinned.needs_fresh,
                EXTRA_SCOPES[pinned.invalidations.saturating_sub(1) % EXTRA_SCOPES.len()],
            )
        };
        let provider = session.token_provider();
        if needs_fresh {
            match provider.get_token(&format!("{SCOPES},{extra}")).await {
                Ok(token) => {
                    let mut pinned = self.pinned.lock().unwrap();
                    pinned.needs_fresh = false;
                    pinned.token = Some(token.access_token.clone());
                    return Ok(token.access_token);
                }
                Err(e) => tracing::warn!("fresh token request failed, using cached scopes: {e}"),
            }
        }
        provider
            .get_token(SCOPES)
            .await
            .map(|t| t.access_token)
            .map_err(|e| {
                tracing::warn!("token request failed: {e}");
                FetchError::Offline
            })
    }

    async fn token_within(&self, limit: Duration) -> Result<String, FetchError> {
        tokio::time::timeout(limit, self.fetch())
            .await
            .unwrap_or(Err(FetchError::Offline))
    }
}

impl TokenSource for SessionTokens {
    async fn token(&self) -> Result<String, FetchError> {
        self.token_within(TOKEN_TIMEOUT).await
    }

    fn invalidate(&self) {
        let mut pinned = self.pinned.lock().unwrap();
        pinned.token = None;
        pinned.needs_fresh = true;
        pinned.invalidations += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dropped_session_sender_is_an_auth_failure() {
        let (tx, rx) = watch::channel(None);
        let tokens = SessionTokens::new(rx);
        drop(tx);
        assert_eq!(tokens.token().await, Err(FetchError::Auth));
    }

    #[tokio::test]
    async fn waiting_for_a_session_times_out_as_offline() {
        let (_tx, rx) = watch::channel(None);
        let tokens = SessionTokens::new(rx);
        assert_eq!(
            tokens.token_within(Duration::from_millis(50)).await,
            Err(FetchError::Offline)
        );
    }

    #[test]
    fn invalidate_clears_the_pinned_token_and_cycles_scopes() {
        let (_tx, rx) = watch::channel(None);
        let tokens = SessionTokens::new(rx);
        tokens.pinned.lock().unwrap().token = Some("old".into());
        tokens.invalidate();
        let p = tokens.pinned.lock().unwrap();
        assert!(p.token.is_none() && p.needs_fresh && p.invalidations == 1);
    }
}
