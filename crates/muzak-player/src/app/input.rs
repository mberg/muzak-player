use crate::app::state::DisplayMode;
use crate::model::{
    Account, Collection, EditOutcome, PlaylistEdit, Repeat, SearchResults, Section, Track,
};

/// Everything the core reacts to.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Ui(UiAction),
    Player(PlayerUpdate),
    Library(LibraryUpdate),
    Speaker {
        connected: bool,
    },
    AuthInvalid,
    /// Sent once a second by the runtime.
    Tick,
    /// Sent every 150ms while the search screen is open, to send a search soon after typing stops.
    SearchTick,
    Bluetooth(BtUpdate),
}

/// A Bluetooth audio device seen in a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundSpeaker {
    pub address: String,
    pub name: String,
    pub paired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtUpdate {
    Found(FoundSpeaker),
    ScanFinished,
    /// Paired, trusted and connected; the address.
    Connected(String),
    ConnectFailed(String),
    Forgotten(String),
    /// No Bluetooth adapter or service on this device.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtCommand {
    Scan,
    Connect(String),
    Forget(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    ShowSection(Section),
    OpenCollection(String),
    Back,
    OpenNowPlaying,
    PlayCollection {
        uri: String,
        shuffle: bool,
    },
    PlayTrack {
        collection_uri: String,
        index: usize,
    },
    TogglePlay,
    Next,
    Previous,
    Seek {
        position_ms: u32,
    },
    SetVolume {
        percent: u8,
    },
    ToggleShuffle,
    CycleRepeat,
    /// A tap on the dim/off overlay; only wakes the screen.
    Touch,
    /// Text typed on the on-screen or physical keyboard.
    KeyPressed(String),
    Backspace,
    ClearSearch,
    OpenKeyboard,
    CloseKeyboard,
    /// Leave search and go back to the section that was open before.
    CloseSearch,
    SetSearchFilter(crate::app::state::SearchFilter),
    OpenArtist(String),
    /// From Now Playing: open the playing song's artist.
    OpenPlayingArtist,
    /// From an album page: open the album's artist (or go back to them in a drill-down).
    OpenAlbumArtist(String),
    PlayArtist(String),
    /// Play one song picked from search results, by track URI.
    PlaySong(String),
    /// Open the add-to-playlist picker for this track URI.
    OpenPicker(String),
    ClosePicker,
    PickPlaylist(String),
    /// From the picker: name a new playlist for the song.
    NewPlaylist,
    /// The keyboard's Done key: saves a name, or hides the search keyboard.
    KeyboardDone,
    CancelText,
    EditPlaylist(String),
    FinishEditing,
    RemoveTrack(usize),
    MoveTrack {
        from: usize,
        to: usize,
    },
    RenamePlaylist,
    AskDelete,
    ConfirmDelete,
    CancelDelete,
    /// Like or unlike the playing song.
    ToggleLike,
    /// Switch collection screens between tiles and a list.
    ToggleListView,
    /// From the add sheet: put the song in Liked Songs.
    PickLiked,
    /// Save the album to the library, or remove it.
    ToggleSaveAlbum(String),
    /// Follow the artist, or stop following.
    ToggleFollow(String),
    /// Settings: name this device with the keyboard.
    RenameDevice,
    FindSpeakers,
    ConnectSpeaker(String),
    UseJack,
    ForgetSpeaker,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerUpdate {
    Connected,
    Disconnected,
    TrackChanged(Track),
    Loading,
    Playing {
        position_ms: u32,
    },
    Paused {
        position_ms: u32,
    },
    Stopped,
    Position {
        position_ms: u32,
    },
    Volume {
        percent: u8,
    },
    Shuffle(bool),
    Repeat(Repeat),
    /// librespot skips unavailable tracks itself; this only drives a notice.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LibraryUpdate {
    Section {
        section: Section,
        items: Vec<Collection>,
    },
    Tracks {
        collection_uri: String,
        tracks: Vec<Track>,
    },
    SectionFailed {
        section: Section,
        reason: FailReason,
    },
    TracksFailed {
        collection_uri: String,
        reason: FailReason,
    },
    Account(Account),
    SearchResults {
        query: String,
        results: SearchResults,
    },
    SearchFailed {
        query: String,
        reason: FailReason,
    },
    ArtistAlbums {
        artist_uri: String,
        albums: Vec<Collection>,
    },
    ArtistAlbumsFailed {
        artist_uri: String,
        reason: FailReason,
    },
    EditDone {
        id: u64,
        outcome: EditOutcome,
    },
    EditFailed {
        id: u64,
        reason: FailReason,
    },
    Liked {
        track_uri: String,
        liked: bool,
    },
    ArtistFound {
        album_uri: String,
        artist: Collection,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    Offline,
    Auth,
    /// Spotify refuses this to the app; playing it may still work.
    Forbidden,
    /// Spotify asked the app to stop for a while.
    RateLimited,
    Other,
}

/// Work the runtime must carry out after a state change.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Player(PlayerCommand),
    Library(LibraryRequest),
    Display(DisplayMode),
    Bluetooth(BtCommand),
    /// Save the settings and restart the player so they take effect.
    ApplySettings(crate::settings::Settings),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerCommand {
    Load {
        context_uri: String,
        start_index: Option<u32>,
        /// When set, playback starts at this track instead of `start_index`.
        start_uri: Option<String>,
        shuffle: bool,
    },
    Play,
    Pause,
    Next,
    Previous,
    Seek {
        position_ms: u32,
    },
    SetVolume {
        percent: u8,
    },
    SetShuffle(bool),
    SetRepeat(Repeat),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LibraryRequest {
    Section(Section),
    Tracks {
        collection_uri: String,
    },
    Account,
    Search(String),
    /// A section fetched even if the cache is fresh, after an edit changed it.
    Reload(Section),
    /// A track list fetched even if the cache is fresh, after an edit changed it.
    ReloadTracks {
        collection_uri: String,
    },
    ArtistAlbums {
        artist_uri: String,
    },
    /// A playlist change; `id` matches the reply to the core's undo record.
    Edit {
        id: u64,
        edit: PlaylistEdit,
    },
    IsLiked {
        track_uri: String,
    },
    /// Look up the artist of an album that was cached without one.
    FindArtist {
        album_uri: String,
        name: String,
    },
}
