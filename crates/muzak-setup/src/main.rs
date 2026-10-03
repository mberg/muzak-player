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
    /// Check the saved credentials can read every Web API endpoint the player uses.
    Probe {
        #[arg(long)]
        state_dir: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Auth { state_dir } => auth(&state_dir).await,
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

async fn probe(state_dir: &Path) -> anyhow::Result<()> {
    let (_, cache) = cache(state_dir)?;
    let credentials = cache
        .credentials()
        .context("no saved credentials; run `muzak-setup auth` first")?;
    let session = Session::new(SessionConfig::default(), Some(cache));
    session.connect(credentials, true).await?;
    println!("Connected as {}", session.username());
    let token = session.token_provider().get_token(SCOPES).await?;
    let client = reqwest::Client::new();

    let get = |path: String| {
        let request = client
            .get(format!("{API}{path}"))
            .bearer_auth(&token.access_token);
        async move {
            let response = request.send().await?;
            let status = response.status();
            let body: serde_json::Value = response.json().await.unwrap_or_default();
            anyhow::Ok((status, body))
        }
    };

    let mut failures = 0;
    let mut first_playlist = None;
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
            first_playlist = body["items"][0]["id"].as_str().map(str::to_string);
        }
    }
    if let Some(id) = first_playlist {
        let mut any_ok = false;
        for path in [
            format!("/playlists/{id}/items?limit=5"),
            format!("/playlists/{id}/tracks?limit=5"),
        ] {
            let (status, _) = get(path.clone()).await?;
            println!("{:<16} HTTP {status}  {path}", "playlist items");
            any_ok |= status.is_success();
        }
        if !any_ok {
            failures += 1;
        }
    }
    if failures > 0 {
        bail!("{failures} endpoint group(s) failed; apply Contingency A in the phase 1 plan");
    }
    println!("All endpoints work with librespot tokens.");
    Ok(())
}
