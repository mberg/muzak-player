use crate::app::state::DisplayMode;
use crate::model::{Collection, Repeat, Section, Track};

/// Everything the core reacts to.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Ui(UiAction),
    Player(PlayerUpdate),
    Library(LibraryUpdate),
    Speaker { connected: bool },
    AuthInvalid,
    /// Sent once a second by the runtime.
    Tick,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    ShowSection(Section),
    OpenCollection(String),
    Back,
    OpenNowPlaying,
    PlayCollection { uri: String, shuffle: bool },
    PlayTrack { collection_uri: String, index: usize },
    TogglePlay,
    Next,
    Previous,
    Seek { position_ms: u32 },
    SetVolume { percent: u8 },
    ToggleShuffle,
    CycleRepeat,
    /// A tap on the dim/off overlay; only wakes the screen.
    Touch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerUpdate {
    Connected,
    Disconnected,
    TrackChanged(Track),
    Loading,
    Playing { position_ms: u32 },
    Paused { position_ms: u32 },
    Stopped,
    Position { position_ms: u32 },
    Volume { percent: u8 },
    Shuffle(bool),
    Repeat(Repeat),
    /// librespot skips unavailable tracks itself; this only drives a notice.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LibraryUpdate {
    Section { section: Section, items: Vec<Collection> },
    Tracks { collection_uri: String, tracks: Vec<Track> },
    SectionFailed { section: Section, reason: FailReason },
    TracksFailed { collection_uri: String, reason: FailReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    Offline,
    Auth,
    Other,
}

/// Work the runtime must carry out after a state change.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Player(PlayerCommand),
    Library(LibraryRequest),
    Display(DisplayMode),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerCommand {
    Load { context_uri: String, start_index: Option<u32>, shuffle: bool },
    Play,
    Pause,
    Next,
    Previous,
    Seek { position_ms: u32 },
    SetVolume { percent: u8 },
    SetShuffle(bool),
    SetRepeat(Repeat),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LibraryRequest {
    Section(Section),
    Tracks { collection_uri: String },
}
