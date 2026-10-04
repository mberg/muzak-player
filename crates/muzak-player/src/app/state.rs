use std::collections::HashMap;
use std::sync::Arc;

use crate::model::{Account, Collection, Repeat, SearchResults, Section, Track};

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Grid(Section),
    Detail(String),
    NowPlaying,
    Search,
    /// An artist's albums, by artist URI.
    Artist(String),
}

/// Library data plus its loading status. Data stays visible while a refresh runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot<T> {
    pub data: Option<Arc<T>>,
    pub loading: bool,
    /// True only when a fetch failed and there is no data to show.
    pub failed: bool,
    /// True when Spotify refused the fetch (HTTP 403) and there is no data to show.
    pub forbidden: bool,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Slot {
            data: None,
            loading: false,
            failed: false,
            forbidden: false,
        }
    }
}

/// Which kinds of results the search screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchFilter {
    #[default]
    All,
    Songs,
    Artists,
    Albums,
    Playlists,
}

impl SearchFilter {
    pub fn index(self) -> i32 {
        match self {
            SearchFilter::All => 0,
            SearchFilter::Songs => 1,
            SearchFilter::Artists => 2,
            SearchFilter::Albums => 3,
            SearchFilter::Playlists => 4,
        }
    }

    pub fn from_index(index: i32) -> SearchFilter {
        match index {
            1 => SearchFilter::Songs,
            2 => SearchFilter::Artists,
            3 => SearchFilter::Albums,
            4 => SearchFilter::Playlists,
            _ => SearchFilter::All,
        }
    }
}

/// The search box and its catalog results.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SearchState {
    pub query: String,
    /// The query last sent to the catalog; results belong to it.
    pub sent: String,
    /// When `query` last changed.
    pub edited_ms: u64,
    pub results: Slot<SearchResults>,
    pub filter: SearchFilter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayStatus {
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    pub track: Option<Track>,
    pub context_uri: Option<String>,
    pub status: PlayStatus,
    pub position_ms: u32,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    NoInternet,
    TrackUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Active,
    Dim,
    Off,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub screen: Screen,
    /// The rail item that is highlighted.
    pub section: Section,
    pub back_stack: Vec<Screen>,
    pub sections: HashMap<Section, Slot<Vec<Collection>>>,
    /// Track lists keyed by collection URI.
    pub tracks: HashMap<String, Slot<Vec<Track>>>,
    pub playback: Playback,
    pub notice: Option<Notice>,
    pub display: DisplayMode,
    pub online: bool,
    pub auth_needed: bool,
    pub speaker_connected: bool,
    /// The account the library comes from, once known.
    pub account: Option<Account>,
    pub search: SearchState,
    pub keyboard_open: bool,
    /// Album lists keyed by artist URI.
    pub artist_albums: HashMap<String, Slot<Vec<Collection>>>,
}
