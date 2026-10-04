pub mod cache;
pub mod fake;
pub mod refresh_tokens;
pub mod service;
pub mod session_tokens;
pub mod web_api;

use std::future::Future;

use crate::app::FailReason;
use crate::model::{Account, Collection, SearchResults, Section, Track};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FetchError {
    #[error("offline")]
    Offline,
    #[error("not signed in")]
    Auth,
    #[error("not found")]
    NotFound,
    /// Spotify refused this resource to the app (HTTP 403).
    #[error("forbidden")]
    Forbidden,
    #[error("{0}")]
    Other(String),
}

impl FetchError {
    pub fn reason(&self) -> FailReason {
        match self {
            FetchError::Offline => FailReason::Offline,
            FetchError::Auth => FailReason::Auth,
            FetchError::Forbidden => FailReason::Forbidden,
            FetchError::NotFound | FetchError::Other(_) => FailReason::Other,
        }
    }
}

/// Where library data comes from: the Spotify Web API, or the fake catalog.
pub trait LibrarySource: Send + Sync + 'static {
    fn section(
        &self,
        section: Section,
    ) -> impl Future<Output = Result<Vec<Collection>, FetchError>> + Send;
    fn tracks(
        &self,
        collection_uri: &str,
    ) -> impl Future<Output = Result<Vec<Track>, FetchError>> + Send;
    /// Catalog search across songs, artists, albums and playlists.
    fn search(
        &self,
        query: &str,
    ) -> impl Future<Output = Result<SearchResults, FetchError>> + Send {
        let _ = query;
        async { Err(FetchError::NotFound) }
    }
    /// An artist's albums, singles and compilations.
    fn artist_albums(
        &self,
        artist_uri: &str,
    ) -> impl Future<Output = Result<Vec<Collection>, FetchError>> + Send {
        let _ = artist_uri;
        async { Err(FetchError::NotFound) }
    }
    /// The signed-in account, shown in the rail.
    fn account(&self) -> impl Future<Output = Result<Account, FetchError>> + Send {
        async { Err(FetchError::NotFound) }
    }
}
