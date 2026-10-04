use serde::{Deserialize, Serialize};

/// Pseudo-collection URI for the account's Liked Songs. The player maps it to
/// `spotify:user:<username>:collection`, the library to `/me/tracks`.
pub const LIKED_URI: &str = "muzak:liked";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Section {
    Playlists,
    Albums,
    Liked,
    Recent,
    Search,
}

impl Section {
    pub fn index(self) -> i32 {
        match self {
            Section::Playlists => 0,
            Section::Albums => 1,
            Section::Liked => 2,
            Section::Recent => 3,
            Section::Search => 4,
        }
    }

    pub fn from_index(index: i32) -> Section {
        match index {
            1 => Section::Albums,
            2 => Section::Liked,
            3 => Section::Recent,
            4 => Section::Search,
            _ => Section::Playlists,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Playlists => "Playlists",
            Section::Albums => "Albums",
            Section::Liked => "Liked Songs",
            Section::Recent => "Recent",
            Section::Search => "Search",
        }
    }

    pub fn cache_key(self) -> &'static str {
        match self {
            Section::Playlists => "section-playlists",
            Section::Albums => "section-albums",
            Section::Liked => "section-liked",
            Section::Recent => "section-recent",
            Section::Search => "section-search",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CollectionKind {
    #[default]
    Playlist,
    Album,
    Liked,
    Artist,
}

/// Something you can open and play as a whole: a playlist, an album, or Liked Songs.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Collection {
    pub uri: String,
    pub kind: CollectionKind,
    pub name: String,
    /// Owner for playlists, artists for albums.
    pub subtitle: String,
    pub image_url: Option<String>,
    /// Spotify ID of a playlist's owner. Only owned playlists can be edited.
    #[serde(default)]
    pub owner_id: Option<String>,
    /// A playlist's version, sent with edits so Spotify can detect conflicts.
    #[serde(default)]
    pub snapshot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub uri: String,
    pub name: String,
    /// Artist names joined with ", ".
    pub artists: String,
    pub album: String,
    pub image_url: Option<String>,
    pub duration_ms: u32,
    /// The album this track is on, used to play a single song in its album.
    #[serde(default)]
    pub album_uri: Option<String>,
}

/// Catalog search results. Library matches are computed from state by the view.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub artists: Vec<Collection>,
    pub albums: Vec<Collection>,
    pub playlists: Vec<Collection>,
}

/// The Spotify account the library is read from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    /// Spotify display name; falls back to the ID when the account has none.
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    Context,
    Track,
}

impl Repeat {
    pub fn next(self) -> Repeat {
        match self {
            Repeat::Off => Repeat::Context,
            Repeat::Context => Repeat::Track,
            Repeat::Track => Repeat::Off,
        }
    }

    pub fn index(self) -> i32 {
        match self {
            Repeat::Off => 0,
            Repeat::Context => 1,
            Repeat::Track => 2,
        }
    }
}

pub fn liked_collection() -> Collection {
    Collection {
        uri: LIKED_URI.to_string(),
        kind: CollectionKind::Liked,
        name: Section::Liked.title().to_string(),
        subtitle: String::new(),
        image_url: None,
        ..Default::default()
    }
}
