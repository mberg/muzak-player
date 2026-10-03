pub mod input;
pub mod state;
#[cfg(test)]
pub(crate) mod tests;

use std::sync::Arc;

pub use input::*;
pub use state::*;

use crate::model::{LIKED_URI, Section};

/// How long a notice stays on screen.
const NOTICE_MS: u64 = 4_000;
/// Identical play requests closer together than this are treated as one.
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
    cfg: CoreConfig,
    last_activity_ms: u64,
    notice_until_ms: u64,
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
        let mut core = Core {
            state,
            cfg,
            last_activity_ms: now_ms,
            notice_until_ms: 0,
            last_load: None,
        };
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

    fn on_ui(&mut self, action: UiAction, now_ms: u64, fx: &mut Vec<Effect>) {
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
            UiAction::OpenNowPlaying => {
                let pb = &self.state.playback;
                if pb.track.is_some() || pb.context_uri.is_some() {
                    self.navigate(Screen::NowPlaying);
                }
            }
            UiAction::PlayCollection { uri, shuffle } => {
                let start = if shuffle { None } else { Some(0) };
                self.start_playback(uri, start, shuffle, now_ms, fx);
            }
            UiAction::PlayTrack {
                collection_uri,
                index,
            } => {
                let shuffle = self.state.playback.shuffle;
                self.start_playback(collection_uri, Some(index as u32), shuffle, now_ms, fx);
            }
            UiAction::TogglePlay => {
                let pb = &mut self.state.playback;
                let has_something = pb.track.is_some() || pb.context_uri.is_some();
                match pb.status {
                    PlayStatus::Playing | PlayStatus::Loading => {
                        pb.status = PlayStatus::Paused;
                        fx.push(Effect::Player(PlayerCommand::Pause));
                    }
                    PlayStatus::Paused | PlayStatus::Stopped if has_something => {
                        pb.status = PlayStatus::Playing;
                        fx.push(Effect::Player(PlayerCommand::Play));
                    }
                    _ => {}
                }
            }
            UiAction::Next => {
                self.state.playback.position_ms = 0;
                fx.push(Effect::Player(PlayerCommand::Next));
            }
            UiAction::Previous => {
                self.state.playback.position_ms = 0;
                fx.push(Effect::Player(PlayerCommand::Previous));
            }
            UiAction::Seek { position_ms } => {
                let max = self
                    .state
                    .playback
                    .track
                    .as_ref()
                    .map_or(u32::MAX, |t| t.duration_ms);
                let position_ms = position_ms.min(max);
                self.state.playback.position_ms = position_ms;
                fx.push(Effect::Player(PlayerCommand::Seek { position_ms }));
            }
            UiAction::SetVolume { percent } => {
                let percent = percent.min(100);
                self.state.playback.volume = percent;
                fx.push(Effect::Player(PlayerCommand::SetVolume { percent }));
            }
            UiAction::ToggleShuffle => {
                let shuffle = !self.state.playback.shuffle;
                self.state.playback.shuffle = shuffle;
                fx.push(Effect::Player(PlayerCommand::SetShuffle(shuffle)));
            }
            UiAction::CycleRepeat => {
                let repeat = self.state.playback.repeat.next();
                self.state.playback.repeat = repeat;
                fx.push(Effect::Player(PlayerCommand::SetRepeat(repeat)));
            }
            UiAction::Touch => {}
        }
    }

    fn on_player(&mut self, update: PlayerUpdate, now_ms: u64, fx: &mut Vec<Effect>) {
        match update {
            PlayerUpdate::Connected => {
                self.state.online = true;
                self.refresh_after_reconnect(fx);
            }
            PlayerUpdate::Disconnected => {
                self.state.online = false;
                if self.state.playback.status == PlayStatus::Playing {
                    self.state.playback.status = PlayStatus::Paused;
                }
            }
            PlayerUpdate::TrackChanged(track) => {
                self.state.playback.track = Some(track);
                self.state.playback.position_ms = 0;
            }
            PlayerUpdate::Loading => self.state.playback.status = PlayStatus::Loading,
            PlayerUpdate::Playing { position_ms } => {
                self.state.playback.status = PlayStatus::Playing;
                self.state.playback.position_ms = position_ms;
                self.wake(now_ms, fx);
            }
            PlayerUpdate::Paused { position_ms } => {
                self.state.playback.status = PlayStatus::Paused;
                self.state.playback.position_ms = position_ms;
            }
            PlayerUpdate::Stopped => self.state.playback.status = PlayStatus::Stopped,
            PlayerUpdate::Position { position_ms } => self.state.playback.position_ms = position_ms,
            PlayerUpdate::Volume { percent } => self.state.playback.volume = percent.min(100),
            PlayerUpdate::Shuffle(shuffle) => self.state.playback.shuffle = shuffle,
            PlayerUpdate::Repeat(repeat) => self.state.playback.repeat = repeat,
            PlayerUpdate::Unavailable => self.notify(Notice::TrackUnavailable, now_ms),
        }
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
            LibraryUpdate::Tracks {
                collection_uri,
                tracks,
            } => {
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
            LibraryUpdate::TracksFailed {
                collection_uri,
                reason,
            } => {
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

    fn on_tick(&mut self, now_ms: u64, fx: &mut Vec<Effect>) {
        if self.state.notice.is_some() && now_ms >= self.notice_until_ms {
            self.state.notice = None;
        }
        if self.state.playback.status == PlayStatus::Playing {
            self.last_activity_ms = now_ms;
        }
        let idle = now_ms.saturating_sub(self.last_activity_ms);
        let target = if idle >= self.cfg.off_after_ms {
            DisplayMode::Off
        } else if idle >= self.cfg.dim_after_ms {
            DisplayMode::Dim
        } else {
            DisplayMode::Active
        };
        if target != self.state.display {
            self.state.display = target;
            fx.push(Effect::Display(target));
        }
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
        self.state
            .tracks
            .entry(uri.to_string())
            .or_default()
            .loading = true;
        fx.push(Effect::Library(LibraryRequest::Tracks {
            collection_uri: uri.to_string(),
        }));
    }

    fn start_playback(
        &mut self,
        uri: String,
        start_index: Option<u32>,
        shuffle: bool,
        now_ms: u64,
        fx: &mut Vec<Effect>,
    ) {
        let key = (uri.clone(), start_index, shuffle);
        if let Some((last_key, at)) = &self.last_load {
            if *last_key == key && now_ms.saturating_sub(*at) < DOUBLE_TAP_MS {
                return;
            }
        }
        self.last_load = Some((key, now_ms));
        if !self.state.online {
            self.notify(Notice::NoInternet, now_ms);
        }
        let first = start_index.and_then(|index| {
            self.state
                .tracks
                .get(&uri)
                .and_then(|slot| slot.data.as_ref())
                .and_then(|tracks| tracks.get(index as usize))
                .cloned()
        });
        let pb = &mut self.state.playback;
        pb.context_uri = Some(uri.clone());
        pb.track = first;
        pb.status = PlayStatus::Loading;
        pb.position_ms = 0;
        pb.shuffle = shuffle;
        fx.push(Effect::Player(PlayerCommand::Load {
            context_uri: uri,
            start_index,
            shuffle,
        }));
        self.navigate(Screen::NowPlaying);
    }

    fn notify(&mut self, notice: Notice, now_ms: u64) {
        self.state.notice = Some(notice);
        self.notice_until_ms = now_ms + NOTICE_MS;
    }

    fn refresh_after_reconnect(&mut self, fx: &mut Vec<Effect>) {
        for section in [Section::Playlists, Section::Albums, Section::Recent] {
            self.request_section(section, fx);
        }
        let open_detail = std::iter::once(&self.state.screen)
            .chain(self.state.back_stack.iter().rev())
            .find_map(|screen| match screen {
                Screen::Detail(uri) => Some(uri.clone()),
                _ => None,
            });
        if let Some(uri) = open_detail {
            self.request_tracks(&uri, fx);
        }
    }
}
