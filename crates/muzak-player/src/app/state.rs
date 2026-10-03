use std::collections::HashMap;
use std::sync::Arc;

use crate::model::{Collection, Repeat, Section, Track};

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Grid(Section),
    Detail(String),
    NowPlaying,
}

/// Library data plus its loading status. Data stays visible while a refresh runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot<T> {
    pub data: Option<Arc<T>>,
    pub loading: bool,
    /// True only when a fetch failed and there is no data to show.
    pub failed: bool,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Slot {
            data: None,
            loading: false,
            failed: false,
        }
    }
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
}
