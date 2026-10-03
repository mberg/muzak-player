pub mod input;
pub mod state;
#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use input::*;
pub use state::*;

use crate::model::{LIKED_URI, Section};

/// How long a notice stays on screen.
#[allow(dead_code)] // used from Task 3
const NOTICE_MS: u64 = 4_000;
/// Identical play requests closer together than this are treated as one.
#[allow(dead_code)] // used from Task 3
const DOUBLE_TAP_MS: u64 = 1_000;

#[derive(Debug, Clone)]
pub struct CoreConfig {
    pub dim_after_ms: u64,
    pub off_after_ms: u64,
    pub initial_volume: u8,
}

/// The app's state machine. Pure: no I/O, time is passed in.
pub struct Core {
    state: AppState,
    #[allow(dead_code)] // read from Task 3
    cfg: CoreConfig,
    last_activity_ms: u64,
    #[allow(dead_code)] // read from Task 3
    notice_until_ms: u64,
    #[allow(dead_code)] // read from Task 3
    last_load: Option<((String, Option<u32>, bool), u64)>,
}

impl Core {
    pub fn new(cfg: CoreConfig, now_ms: u64) -> (Core, Vec<Effect>) {
        let state = AppState {
            screen: Screen::Grid(Section::Playlists),
            section: Section::Playlists,
            back_stack: Vec::new(),
            sections: Default::default(),
            tracks: Default::default(),
            playback: Playback {
                track: None,
                context_uri: None,
                status: PlayStatus::Stopped,
                position_ms: 0,
                volume: cfg.initial_volume.min(100),
                shuffle: false,
                repeat: Default::default(),
            },
            notice: None,
            display: DisplayMode::Active,
            online: true,
            auth_needed: false,
            speaker_connected: true,
        };
        let mut core = Core { state, cfg, last_activity_ms: now_ms, notice_until_ms: 0, last_load: None };
        let mut fx = Vec::new();
        for section in [Section::Playlists, Section::Albums, Section::Recent] {
            core.request_section(section, &mut fx);
        }
        fx.push(Effect::Display(DisplayMode::Active));
        (core, fx)
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn handle(&mut self, input: Input, now_ms: u64) -> Vec<Effect> {
        let mut fx = Vec::new();
        match input {
            Input::Ui(action) => {
                self.wake(now_ms, &mut fx);
                self.on_ui(action, now_ms, &mut fx);
            }
            Input::Player(update) => self.on_player(update, now_ms, &mut fx),
            Input::Library(update) => self.on_library(update),
            Input::Speaker { connected } => self.state.speaker_connected = connected,
            Input::AuthInvalid => self.state.auth_needed = true,
            Input::Tick => self.on_tick(now_ms, &mut fx),
        }
        fx
    }

    fn on_ui(&mut self, action: UiAction, _now_ms: u64, fx: &mut Vec<Effect>) {
        match action {
            UiAction::ShowSection(Section::Liked) => {
                self.state.section = Section::Liked;
                self.state.back_stack.clear();
                self.state.screen = Screen::Detail(LIKED_URI.to_string());
                self.request_tracks(LIKED_URI, fx);
            }
            UiAction::ShowSection(section) => {
                self.state.section = section;
                self.state.back_stack.clear();
                self.state.screen = Screen::Grid(section);
                self.request_section(section, fx);
            }
            UiAction::OpenCollection(uri) => {
                self.navigate(Screen::Detail(uri.clone()));
                self.request_tracks(&uri, fx);
            }
            UiAction::Back => {
                if let Some(previous) = self.state.back_stack.pop() {
                    self.state.screen = previous;
                }
            }
            // Playback actions are implemented in Task 3.
            _ => {}
        }
    }

    fn on_player(&mut self, _update: PlayerUpdate, _now_ms: u64, _fx: &mut Vec<Effect>) {
        // Implemented in Task 3.
    }

    fn on_library(&mut self, update: LibraryUpdate) {
        match update {
            LibraryUpdate::Section { section, items } => {
                let slot = self.state.sections.entry(section).or_default();
                slot.data = Some(Arc::new(items));
                slot.loading = false;
                slot.failed = false;
                self.state.online = true;
            }
            LibraryUpdate::Tracks { collection_uri, tracks } => {
                let slot = self.state.tracks.entry(collection_uri).or_default();
                slot.data = Some(Arc::new(tracks));
                slot.loading = false;
                slot.failed = false;
                self.state.online = true;
            }
            LibraryUpdate::SectionFailed { section, reason } => {
                let slot = self.state.sections.entry(section).or_default();
                slot.loading = false;
                slot.failed = slot.data.is_none();
                self.on_failure(reason);
            }
            LibraryUpdate::TracksFailed { collection_uri, reason } => {
                let slot = self.state.tracks.entry(collection_uri).or_default();
                slot.loading = false;
                slot.failed = slot.data.is_none();
                self.on_failure(reason);
            }
        }
    }

    fn on_failure(&mut self, reason: FailReason) {
        match reason {
            FailReason::Offline => self.state.online = false,
            FailReason::Auth => self.state.auth_needed = true,
            FailReason::Other => {}
        }
    }

    fn on_tick(&mut self, _now_ms: u64, _fx: &mut Vec<Effect>) {
        // Implemented in Task 3.
    }

    fn wake(&mut self, now_ms: u64, fx: &mut Vec<Effect>) {
        self.last_activity_ms = now_ms;
        if self.state.display != DisplayMode::Active {
            self.state.display = DisplayMode::Active;
            fx.push(Effect::Display(DisplayMode::Active));
        }
    }

    fn navigate(&mut self, screen: Screen) {
        if self.state.screen != screen {
            let previous = std::mem::replace(&mut self.state.screen, screen);
            self.state.back_stack.push(previous);
        }
    }

    fn request_section(&mut self, section: Section, fx: &mut Vec<Effect>) {
        self.state.sections.entry(section).or_default().loading = true;
        fx.push(Effect::Library(LibraryRequest::Section(section)));
    }

    fn request_tracks(&mut self, uri: &str, fx: &mut Vec<Effect>) {
        self.state.tracks.entry(uri.to_string()).or_default().loading = true;
        fx.push(Effect::Library(LibraryRequest::Tracks { collection_uri: uri.to_string() }));
    }
}
