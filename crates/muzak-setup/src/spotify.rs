//! Spotify sign-in for a device: playback (librespot) and the library (the developer app).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use librespot::core::authentication::Credentials;
use librespot::core::cache::Cache;
use librespot::core::config::SessionConfig;
use librespot::core::session::Session;
use librespot::oauth::OAuthClientBuilder;

pub const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
/// Keep in sync with `crates/muzak-player/src/library/web_api.rs`.
const SCOPES: &str = "playlist-read-private,playlist-read-collaborative,user-library-read,user-read-recently-played,playlist-modify-private,playlist-modify-public,user-library-modify,user-follow-read,user-follow-modify";
const API: &str = "https://api.spotify.com/v1";

fn cache(state_dir: &Path) -> anyhow::Result<(PathBuf, Cache)> {
    let dir = state_dir.join("librespot");
    std::fs::create_dir_all(&dir)?;
    let cache = Cache::new(Some(&dir), Some(&dir), None, None)?;
    Ok((dir, cache))
}

pub async fn auth(state_dir: &Path) -> anyhow::Result<()> {
    let session_config = SessionConfig::default();
    let client_id = session_config.client_id.clone();
    println!("Opening the Spotify sign-in page. Sign in with the account this device should use.");
    let token = tokio::task::spawn_blocking(move || {
        OAuthClientBuilder::new(&client_id, REDIRECT_URI, vec!["streaming"])
            .open_in_browser()
            .build()?
            .get_access_token()
    })
    .await?
    .map_err(|e| anyhow!("Spotify sign-in failed: {e}"))?;

    let (dir, cache) = cache(state_dir)?;
    let session = Session::new(session_config, Some(cache));
    session
        .connect(Credentials::with_access_token(token.access_token), true)
        .await?;
    let credentials = dir.join("credentials.json");
    std::fs::set_permissions(&credentials, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting {}", credentials.display()))?;
    println!(
        "Signed in as {}. Saved {}",
        session.username(),
        credentials.display()
    );
    Ok(())
}

pub const WEB_AUTH_FILE: &str = "web-auth.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct WebAuth {
    client_id: String,
    refresh_token: String,
}

pub async fn auth_web(state_dir: &Path, client_id: &str) -> anyhow::Result<()> {
    let id = client_id.to_string();
    println!("Opening the Spotify sign-in page. Sign in with the account this device should use.");
    let token = tokio::task::spawn_blocking(move || {
        OAuthClientBuilder::new(&id, REDIRECT_URI, SCOPES.split(',').collect())
            .open_in_browser()
            .build()?
            .get_access_token()
    })
    .await?
    .map_err(|e| anyhow!("Spotify sign-in failed: {e}"))?;
    if token.refresh_token.is_empty() {
        bail!("Spotify returned no refresh token");
    }
    std::fs::create_dir_all(state_dir)?;
    let path = state_dir.join(WEB_AUTH_FILE);
    let auth = WebAuth {
        client_id: client_id.to_string(),
        refresh_token: token.refresh_token,
    };
    std::fs::write(&path, serde_json::to_vec_pretty(&auth)?)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting {}", path.display()))?;
    println!("Saved {}", path.display());

    let (id, name) = account(&token.access_token).await?;
    println!("Library account: {name} ({id})");
    if let Some(playback) = playback_account(state_dir)
        && playback != id
    {
        println!(
            "WARNING: playback is signed in as {playback}, but the library is {id}. \
             Run `auth` and `auth-web` with the same Spotify account."
        );
    }
    Ok(())
}

/// The Spotify ID and display name behind a Web API access token.
async fn account(access_token: &str) -> anyhow::Result<(String, String)> {
    let me: serde_json::Value = reqwest::Client::new()
        .get(format!("{API}/me"))
        .bearer_auth(access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let id = me["id"].as_str().unwrap_or("?").to_string();
    let name = me["display_name"]
        .as_str()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or(&id)
        .to_string();
    Ok((id, name))
}

/// The username saved by `auth` for playback, if any.
pub fn playback_account(state_dir: &Path) -> Option<String> {
    let dir = state_dir.join("librespot");
    if !dir.join("credentials.json").is_file() {
        return None;
    }
    Cache::new(Some(&dir), None, None, None)
        .ok()?
        .credentials()?
        .username
}

/// A Web API access token from the developer app when `web-auth.json` exists, else from the
/// librespot session. Returns the token and a label for the final message.
async fn access_token(state_dir: &Path) -> anyhow::Result<(String, &'static str)> {
    let web_auth = state_dir.join(WEB_AUTH_FILE);
    if web_auth.is_file() {
        let auth: WebAuth = serde_json::from_slice(&std::fs::read(&web_auth)?)
            .with_context(|| format!("reading {}", web_auth.display()))?;
        println!(
            "Using developer app {} from {}",
            auth.client_id,
            web_auth.display()
        );
        let token = tokio::task::spawn_blocking(move || {
            OAuthClientBuilder::new(&auth.client_id, REDIRECT_URI, SCOPES.split(',').collect())
                .build()?
                .refresh_token(&auth.refresh_token)
        })
        .await?
        .map_err(|e| anyhow!("token refresh failed: {e}"))?;
        return Ok((token.access_token, "developer-app tokens"));
    }

    let (_, cache) = cache(state_dir)?;
    let credentials = cache
        .credentials()
        .context("no saved credentials; sign in first")?;
    let session = Session::new(SessionConfig::default(), Some(cache));
    session.connect(credentials, true).await?;
    println!("Connected as {}", session.username());
    let token = session
        .token_provider()
        .get_token(SCOPES)
        .await
        .context("Spotify refused a Web API token for the librespot session; apply Contingency A (muzak-setup auth-web)")?;
    Ok((token.access_token, "librespot tokens"))
}

pub async fn probe(state_dir: &Path) -> anyhow::Result<()> {
    let (access_token, source) = access_token(state_dir).await?;
    let client = reqwest::Client::new();

    let (library_id, library_name) = account(&access_token).await?;
    let playback = playback_account(state_dir);
    println!("Library account:  {library_name} ({library_id})");
    println!(
        "Playback account: {}",
        playback.as_deref().unwrap_or("none (run `auth`)")
    );
    let mismatch = playback.as_ref().is_some_and(|p| *p != library_id);
    if mismatch {
        println!(
            "WARNING: the library and playback are signed in to different accounts. \
             Run `auth` and `auth-web` with the same Spotify account."
        );
    }

    let get = |path: String| {
        let request = client
            .get(format!("{API}{path}"))
            .bearer_auth(&access_token);
        async move {
            let response = request.send().await?;
            let status = response.status();
            let body: serde_json::Value = response.json().await.unwrap_or_default();
            anyhow::Ok((status, body))
        }
    };

    let mut failures = 0;
    let mut playlists: Vec<(String, String, String)> = Vec::new();
    for (label, path) in [
        ("playlists", "/me/playlists?limit=5"),
        ("saved albums", "/me/albums?limit=5"),
        ("liked songs", "/me/tracks?limit=5"),
        ("recently played", "/me/player/recently-played?limit=5"),
    ] {
        let (status, body) = get(path.to_string()).await?;
        let count = body["items"].as_array().map_or(0, |a| a.len());
        println!("{label:<16} HTTP {status}  items: {count}");
        if !status.is_success() {
            failures += 1;
        }
        if label == "playlists" {
            for item in body["items"].as_array().into_iter().flatten() {
                let field = |key: &str| item[key].as_str().unwrap_or("?").to_string();
                playlists.push((
                    field("id"),
                    field("name"),
                    item["owner"]["id"].as_str().unwrap_or("?").to_string(),
                ));
            }
        }
    }
    // Spotify refuses playlist tracks for playlists it owns itself (Discover Weekly, editorial
    // mixes) to developer apps, so check each playlist and show who owns it.
    let mut any_playlist_ok = false;
    for (id, name, owner) in &playlists {
        let mut any_ok = false;
        for path in [
            format!("/playlists/{id}/items?limit=5"),
            format!("/playlists/{id}/tracks?limit=5"),
        ] {
            let (status, _) = get(path.clone()).await?;
            println!("{:<16} HTTP {status}  {path}", "playlist items");
            any_ok |= status.is_success();
        }
        println!(
            "{:<16} {name:?} owned by {owner}: {}",
            "",
            if any_ok { "ok" } else { "FORBIDDEN" }
        );
        any_playlist_ok |= any_ok;
    }
    if !playlists.is_empty() && !any_playlist_ok {
        failures += 1;
    }
    if mismatch {
        failures += 1;
    }
    if failures > 0 {
        bail!("{failures} endpoint group(s) failed");
    }
    println!("All endpoints work with {source}.");
    Ok(())
}
