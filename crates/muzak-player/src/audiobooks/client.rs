//! A small Audiobookshelf client: sign-in, staying signed in, and the library calls.
//!
//! Since 2.26 Audiobookshelf hands out a short-lived access token and a refresh token
//! (asked for with `x-return-tokens: true`). Refresh tokens rotate on every refresh, so
//! each new one is saved straight away.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::types::{
    BookDetail, BookProgress, BooksLibrary, parse_continue_ids, parse_detail, parse_items,
    parse_progress,
};

pub const AUTH_FILE: &str = "abs-auth.json";
/// Books are fetched this many at a time.
const PAGE: usize = 200;
/// Stop paging after this many books; a home library is far smaller.
const MAX_BOOKS: usize = 5_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbsAuth {
    /// "http://nas.local:13378", no trailing slash.
    pub url: String,
    pub username: String,
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AbsError {
    #[error("can't reach the Audiobookshelf server")]
    Offline,
    #[error("wrong username or password")]
    BadLogin,
    #[error("signed out; sign in again")]
    SignedOut,
    #[error("{0}")]
    Other(String),
}

impl From<reqwest::Error> for AbsError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_connect() || e.is_timeout() {
            AbsError::Offline
        } else {
            AbsError::Other(e.to_string())
        }
    }
}

pub fn normalize_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("http://{url}")
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .expect("HTTP client")
}

fn tokens(body: &Value) -> Option<(String, String)> {
    let user = &body["user"];
    Some((
        user["accessToken"].as_str()?.to_string(),
        user["refreshToken"].as_str()?.to_string(),
    ))
}

/// Signs in with a username and password.
pub async fn login(url: &str, username: &str, password: &str) -> Result<AbsAuth, AbsError> {
    let url = normalize_url(url);
    let response = http()
        .post(format!("{url}/login"))
        .header("x-return-tokens", "true")
        .json(&json!({"username": username, "password": password}))
        .send()
        .await?;
    if response.status().as_u16() == 401 {
        return Err(AbsError::BadLogin);
    }
    if !response.status().is_success() {
        return Err(AbsError::Other(format!("sign-in failed: HTTP {}", response.status())));
    }
    let body: Value = response.json().await?;
    let (access_token, refresh_token) =
        tokens(&body).ok_or_else(|| AbsError::Other("server returned no tokens".into()))?;
    Ok(AbsAuth {
        url,
        username: body["user"]["username"].as_str().unwrap_or(username).to_string(),
        access_token,
        refresh_token,
    })
}

pub fn save_auth(path: &Path, auth: &AbsAuth) -> std::io::Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(auth)?)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

pub struct AbsClient {
    http: reqwest::Client,
    auth: Mutex<AbsAuth>,
    file: PathBuf,
}

impl AbsClient {
    pub fn new(auth: AbsAuth, file: PathBuf) -> Self {
        Self {
            http: http(),
            auth: Mutex::new(auth),
            file,
        }
    }

    pub fn load(file: &Path) -> Option<Self> {
        let auth: AbsAuth = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
        Some(Self::new(auth, file.to_path_buf()))
    }

    pub fn username(&self) -> String {
        self.auth.lock().unwrap().username.clone()
    }

    pub fn url(&self) -> String {
        self.auth.lock().unwrap().url.clone()
    }

    /// Swaps in new tokens, saving the rotated refresh token.
    async fn refresh(&self) -> Result<(), AbsError> {
        let (url, refresh) = {
            let auth = self.auth.lock().unwrap();
            (auth.url.clone(), auth.refresh_token.clone())
        };
        let response = self
            .http
            .post(format!("{url}/auth/refresh"))
            .header("x-refresh-token", refresh)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(AbsError::SignedOut);
        }
        let body: Value = response.json().await?;
        let (access, refresh) = tokens(&body).ok_or(AbsError::SignedOut)?;
        let auth = {
            let mut auth = self.auth.lock().unwrap();
            auth.access_token = access;
            auth.refresh_token = refresh;
            auth.clone()
        };
        if let Err(e) = save_auth(&self.file, &auth) {
            tracing::warn!("saving Audiobookshelf sign-in failed: {e}");
        }
        Ok(())
    }

    /// An authorised request; a 401 refreshes the tokens once and retries.
    async fn send(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<reqwest::Response, AbsError> {
        let mut refreshed = false;
        loop {
            let (url, token) = {
                let auth = self.auth.lock().unwrap();
                (auth.url.clone(), auth.access_token.clone())
            };
            let mut request = self
                .http
                .request(method.clone(), format!("{url}{path}"))
                .bearer_auth(token);
            if let Some(body) = body {
                request = request.json(body);
            }
            let response = request.send().await?;
            if response.status().as_u16() == 401 && !refreshed {
                refreshed = true;
                self.refresh().await?;
                continue;
            }
            return Ok(response);
        }
    }

    async fn get_json(&self, path: &str) -> Result<Option<Value>, AbsError> {
        let response = self.send(reqwest::Method::GET, path, None).await?;
        match response.status().as_u16() {
            404 => Ok(None),
            401 => Err(AbsError::SignedOut),
            s if !(200..300).contains(&s) => Err(AbsError::Other(format!("HTTP {s} for {path}"))),
            _ => Ok(Some(response.json().await?)),
        }
    }

    /// Every book in every book library, with progress and Continue listening.
    pub async fn library(&self) -> Result<BooksLibrary, AbsError> {
        let libraries = self.get_json("/api/libraries").await?.unwrap_or_default();
        let book_libraries: Vec<String> = libraries["libraries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|l| l["mediaType"] == "book")
            .filter_map(|l| l["id"].as_str().map(str::to_string))
            .collect();
        let mut out = BooksLibrary::default();
        for library in &book_libraries {
            let mut page = 0;
            loop {
                let path = format!(
                    "/api/libraries/{library}/items?limit={PAGE}&page={page}&sort=media.metadata.title&minified=1"
                );
                let Some(json) = self.get_json(&path).await? else { break };
                let (books, total) = parse_items(&json);
                let fetched = books.len();
                out.books.extend(books);
                page += 1;
                if fetched == 0 || page * PAGE >= total || out.books.len() >= MAX_BOOKS {
                    break;
                }
            }
            if let Some(shelves) = self
                .get_json(&format!("/api/libraries/{library}/personalized"))
                .await?
            {
                out.continue_ids.extend(parse_continue_ids(&shelves));
            }
        }
        if let Some(me) = self.get_json("/api/me").await? {
            out.progress = me["mediaProgress"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(parse_progress)
                .collect();
        }
        Ok(out)
    }

    /// One book's page, with where the listener is.
    pub async fn book(&self, id: &str) -> Result<(BookDetail, Option<BookProgress>), AbsError> {
        let item = self
            .get_json(&format!("/api/items/{id}?expanded=1"))
            .await?
            .ok_or_else(|| AbsError::Other("book not found".into()))?;
        let detail = parse_detail(&item).ok_or_else(|| AbsError::Other("unreadable book".into()))?;
        let progress = self
            .get_json(&format!("/api/me/progress/{id}"))
            .await?
            .and_then(|p| parse_progress(&p))
            .map(|(_, p)| p);
        Ok((detail, progress))
    }

    /// A book's cover, if it has one.
    pub async fn cover(&self, id: &str) -> Result<Option<Vec<u8>>, AbsError> {
        let response = self
            .send(reqwest::Method::GET, &format!("/api/items/{id}/cover?width=400"), None)
            .await?;
        if !response.status().is_success() {
            return Ok(None);
        }
        Ok(Some(response.bytes().await?.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_addresses_are_tidied() {
        assert_eq!(normalize_url(" nas.local:13378/ "), "http://nas.local:13378");
        assert_eq!(normalize_url("https://books.home"), "https://books.home");
    }

    /// Against a real server: `ABS_URL=http://127.0.0.1:13378 ABS_USER=sam ABS_PASS=… cargo test -- --ignored live_abs`.
    #[tokio::test]
    #[ignore]
    async fn live_abs() {
        let (Ok(url), Ok(user), Ok(pass)) = (
            std::env::var("ABS_URL"),
            std::env::var("ABS_USER"),
            std::env::var("ABS_PASS"),
        ) else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(AUTH_FILE);
        let auth = login(&url, &user, &pass).await.unwrap();
        assert!(matches!(login(&url, &user, "wrong").await, Err(AbsError::BadLogin)));
        let client = AbsClient::new(auth, file.clone());
        // Force a refresh: an invalid access token gets a 401, then rotates and retries.
        client.auth.lock().unwrap().access_token = "expired".into();
        let library = client.library().await.unwrap();
        assert!(!library.books.is_empty());
        assert!(file.is_file(), "the rotated refresh token was saved");
        let (detail, _) = client.book(&library.books[0].id).await.unwrap();
        assert!(!detail.summary.title.is_empty());
        println!("{} books; first: {} ({} chapters)", library.books.len(), detail.summary.title, detail.chapters.len());
    }
}
