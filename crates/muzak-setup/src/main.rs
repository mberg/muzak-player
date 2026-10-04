//! Mac-side setup: sign a kid's Spotify account in and check Web API access.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use clap::{Parser, Subcommand};
use librespot::core::authentication::Credentials;
use librespot::core::cache::Cache;
use librespot::core::config::SessionConfig;
use librespot::core::session::Session;
use librespot::oauth::OAuthClientBuilder;

const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
/// Keep in sync with `crates/muzak-player/src/library/web_api.rs`.
const SCOPES: &str =
    "playlist-read-private,playlist-read-collaborative,user-library-read,user-read-recently-played";
const API: &str = "https://api.spotify.com/v1";

#[derive(Parser)]
#[command(about = "Muzak setup tools")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Sign in to Spotify in the browser and save device credentials.
    Auth {
        #[arg(long)]
        state_dir: PathBuf,
    },
    /// Contingency A: sign in through your own Spotify developer app and save a refresh
    /// token for Web API (library) requests. Playback still uses `auth`.
    AuthWeb {
        #[arg(long)]
        state_dir: PathBuf,
        /// Client ID from https://developer.spotify.com/dashboard.
        #[arg(long)]
        client_id: String,
    },
    /// Check the saved credentials can read every Web API endpoint the player uses.
    /// Uses `web-auth.json` when present, otherwise the librespot session.
    Probe {
        #[arg(long)]
        state_dir: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // librespot and reqwest pull in different rustls crypto backends, so pick one explicitly.
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow!("rustls crypto provider already installed"))?;
    match Cli::parse().command {
        Command::Auth { state_dir } => auth(&state_dir).await,
        Command::AuthWeb {
            state_dir,
            client_id,
        } => auth_web(&state_dir, &client_id).await,
        Command::Probe { state_dir } => probe(&state_dir).await,
    }
}

fn cache(state_dir: &Path) -> anyhow::Result<(PathBuf, Cache)> {
    let dir = state_dir.join("librespot");
    std::fs::create_dir_all(&dir)?;
    let cache = Cache::new(Some(&dir), Some(&dir), None, None)?;
    Ok((dir, cache))
}

async fn auth(state_dir: &Path) -> anyhow::Result<()> {
    let session_config = SessionConfig::default();
    let client_id = session_config.client_id.clone();
    println!("Opening the Spotify sign-in page. Sign in with the kid's account.");
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

const WEB_AUTH_FILE: &str = "web-auth.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct WebAuth {
    client_id: String,
    refresh_token: String,
}

async fn auth_web(state_dir: &Path, client_id: &str) -> anyhow::Result<()> {
    let id = client_id.to_string();
    println!("Opening the Spotify sign-in page. Sign in with the kid's account.");
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
    Ok(())
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
        .context("no saved credentials; run `muzak-setup auth` first")?;
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

async fn probe(state_dir: &Path) -> anyhow::Result<()> {
    let (access_token, source) = access_token(state_dir).await?;
    let client = reqwest::Client::new();

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
    if failures > 0 {
        bail!("{failures} endpoint group(s) failed; apply Contingency A in the phase 1 plan");
    }
    println!("All endpoints work with {source}.");
    Ok(())
}
