//! Turns `AppState` into plain display data. No Slint types here, so it is unit-testable.

use std::sync::Arc;

use crate::app::{AppState, DisplayMode, Notice, PlayStatus, Screen, Slot};
use crate::model::{Collection, LIKED_URI, Repeat, Section, liked_collection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenView {
    Grid,
    Detail,
    NowPlaying,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadStatus {
    Loading,
    Empty,
    Failed,
    Ready,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TileView {
    pub uri: String,
    pub title: String,
    pub subtitle: String,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackRowView {
    pub title: String,
    pub artists: String,
    pub duration: String,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetailView {
    pub header: TileView,
    pub status: LoadStatus,
    pub tracks: Vec<TrackRowView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NowView {
    pub title: String,
    pub artists: String,
    pub image_url: Option<String>,
    pub playing: bool,
    pub loading: bool,
    pub progress: f32,
    pub position: String,
    pub duration: String,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: Repeat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub screen: ScreenView,
    pub section: Section,
    pub grid_title: String,
    pub grid: Vec<TileView>,
    pub grid_status: LoadStatus,
    pub detail: Option<DetailView>,
    pub now: NowView,
    pub mini_visible: bool,
    pub banner: Option<String>,
    pub display: DisplayMode,
    pub auth_needed: bool,
}

pub fn fmt_ms(ms: u32) -> String {
    let secs = ms / 1000;
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub fn build(state: &AppState) -> View {
    let screen = match state.screen {
        Screen::Grid(_) => ScreenView::Grid,
        Screen::Detail(_) => ScreenView::Detail,
        Screen::NowPlaying => ScreenView::NowPlaying,
    };
    let grid_section = match state.screen {
        Screen::Grid(section) => section,
        _ if state.section == Section::Liked => Section::Playlists,
        _ => state.section,
    };
    let grid_slot = state.sections.get(&grid_section);
    let grid = grid_slot
        .and_then(|slot| slot.data.as_ref())
        .map(|items| items.iter().map(tile).collect())
        .unwrap_or_default();
    let pb = &state.playback;
    let has_playback = pb.track.is_some() || pb.context_uri.is_some();

    View {
        screen,
        section: state.section,
        grid_title: grid_section.title().to_string(),
        grid,
        grid_status: status(grid_slot),
        detail: detail(state),
        now: now(state),
        mini_visible: has_playback && screen != ScreenView::NowPlaying,
        banner: banner(state),
        display: state.display,
        auth_needed: state.auth_needed,
    }
}

fn tile(c: &Collection) -> TileView {
    TileView {
        uri: c.uri.clone(),
        title: c.name.clone(),
        subtitle: c.subtitle.clone(),
        image_url: c.image_url.clone(),
    }
}

fn status<T>(slot: Option<&Slot<Vec<T>>>) -> LoadStatus {
    match slot {
        Some(Slot {
            data: Some(data), ..
        }) if data.is_empty() => LoadStatus::Empty,
        Some(Slot { data: Some(_), .. }) => LoadStatus::Ready,
        Some(Slot { failed: true, .. }) => LoadStatus::Failed,
        _ => LoadStatus::Loading,
    }
}

fn find_collection(state: &AppState, uri: &str) -> Option<Collection> {
    if uri == LIKED_URI {
        return Some(liked_collection());
    }
    state
        .sections
        .values()
        .filter_map(|slot| slot.data.as_ref())
        .flat_map(|items: &Arc<Vec<Collection>>| items.iter())
        .find(|c| c.uri == uri)
        .cloned()
}

fn detail(state: &AppState) -> Option<DetailView> {
    let uri = std::iter::once(&state.screen)
        .chain(state.back_stack.iter().rev())
        .find_map(|screen| match screen {
            Screen::Detail(uri) => Some(uri.clone()),
            _ => None,
        })?;
    let header = find_collection(state, &uri)
        .map(|c| tile(&c))
        .unwrap_or(TileView {
            uri: uri.clone(),
            title: String::new(),
            subtitle: String::new(),
            image_url: None,
        });
    let slot = state.tracks.get(&uri);
    let current_uri = state.playback.track.as_ref().map(|t| t.uri.as_str());
    let tracks = slot
        .and_then(|s| s.data.as_ref())
        .map(|tracks| {
            tracks
                .iter()
                .map(|t| TrackRowView {
                    title: t.name.clone(),
                    artists: t.artists.clone(),
                    duration: fmt_ms(t.duration_ms),
                    current: Some(t.uri.as_str()) == current_uri,
                })
                .collect()
        })
        .unwrap_or_default();
    Some(DetailView {
        header,
        status: status(slot),
        tracks,
    })
}

fn now(state: &AppState) -> NowView {
    let pb = &state.playback;
    let (title, artists, image_url) = match &pb.track {
        Some(t) => (t.name.clone(), t.artists.clone(), t.image_url.clone()),
        None => match pb
            .context_uri
            .as_deref()
            .and_then(|uri| find_collection(state, uri))
        {
            Some(c) => (c.name, String::new(), c.image_url),
            None => (String::new(), String::new(), None),
        },
    };
    let duration_ms = pb.track.as_ref().map_or(0, |t| t.duration_ms);
    let progress = if duration_ms > 0 {
        (pb.position_ms as f32 / duration_ms as f32).min(1.0)
    } else {
        0.0
    };
    NowView {
        title,
        artists,
        image_url,
        playing: pb.status == PlayStatus::Playing,
        loading: pb.status == PlayStatus::Loading,
        progress,
        position: fmt_ms(pb.position_ms),
        duration: fmt_ms(duration_ms),
        volume: pb.volume,
        shuffle: pb.shuffle,
        repeat: pb.repeat,
    }
}

fn banner(state: &AppState) -> Option<String> {
    match state.notice {
        Some(Notice::NoInternet) => Some("No internet right now".into()),
        Some(Notice::TrackUnavailable) => Some("That song can't play, skipping".into()),
        None if !state.speaker_connected => Some("Speaker not connected".into()),
        None => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{core, playlist, track, ui, with_tracks};
    use crate::app::{FailReason, Input, LibraryUpdate, PlayerUpdate, UiAction};
    use crate::model::{LIKED_URI, Section};

    #[test]
    fn fmt_ms_formats_minutes_and_seconds() {
        assert_eq!(fmt_ms(0), "0:00");
        assert_eq!(fmt_ms(65_400), "1:05");
        assert_eq!(fmt_ms(3_600_000), "60:00");
    }

    // Review focus 2: an empty library.
    #[test]
    fn grid_status_covers_loading_empty_failed_ready() {
        let mut c = core();
        assert_eq!(build(c.state()).grid_status, LoadStatus::Loading);
        c.handle(
            Input::Library(LibraryUpdate::Section {
                section: Section::Playlists,
                items: vec![],
            }),
            0,
        );
        assert_eq!(build(c.state()).grid_status, LoadStatus::Empty);
        c.handle(
            Input::Library(LibraryUpdate::Section {
                section: Section::Playlists,
                items: vec![playlist(1)],
            }),
            0,
        );
        let v = build(c.state());
        assert_eq!(v.grid_status, LoadStatus::Ready);
        assert_eq!(v.grid[0].title, "Playlist 1");
        assert_eq!(v.grid[0].subtitle, "Mum");
        assert_eq!(v.grid_title, "Playlists");
        c.handle(
            Input::Library(LibraryUpdate::SectionFailed {
                section: Section::Albums,
                reason: FailReason::Other,
            }),
            0,
        );
        c.handle(ui(UiAction::ShowSection(Section::Albums)), 0);
        c.handle(
            Input::Library(LibraryUpdate::SectionFailed {
                section: Section::Albums,
                reason: FailReason::Other,
            }),
            0,
        );
        assert_eq!(build(c.state()).grid_status, LoadStatus::Failed);
    }

    #[test]
    fn detail_shows_collection_and_marks_current_track() {
        let mut c = core();
        c.handle(
            Input::Library(LibraryUpdate::Section {
                section: Section::Playlists,
                items: vec![playlist(1)],
            }),
            0,
        );
        with_tracks(&mut c, "spotify:playlist:p1", 3);
        c.handle(
            ui(UiAction::OpenCollection("spotify:playlist:p1".into())),
            0,
        );
        c.handle(Input::Player(PlayerUpdate::TrackChanged(track(1))), 0);
        let v = build(c.state());
        assert_eq!(v.screen, ScreenView::Detail);
        let d = v.detail.unwrap();
        assert_eq!(d.header.title, "Playlist 1");
        assert_eq!(d.status, LoadStatus::Ready);
        assert_eq!(d.tracks.len(), 3);
        assert_eq!(d.tracks[1].duration, "3:00");
        assert!(d.tracks[1].current && !d.tracks[0].current);
    }

    #[test]
    fn liked_detail_has_title_and_rail_highlights_liked() {
        let mut c = core();
        c.handle(ui(UiAction::ShowSection(Section::Liked)), 0);
        let v = build(c.state());
        assert_eq!(v.section, Section::Liked);
        let d = v.detail.unwrap();
        assert_eq!(d.header.uri, LIKED_URI);
        assert_eq!(d.header.title, "Liked Songs");
        assert_eq!(d.status, LoadStatus::Loading);
    }

    #[test]
    fn now_playing_progress_and_loading_fallback() {
        let mut c = core();
        c.handle(
            Input::Library(LibraryUpdate::Section {
                section: Section::Playlists,
                items: vec![playlist(1)],
            }),
            0,
        );
        c.handle(
            ui(UiAction::PlayCollection {
                uri: "spotify:playlist:p1".into(),
                shuffle: true,
            }),
            0,
        );
        let v = build(c.state());
        assert_eq!(v.screen, ScreenView::NowPlaying);
        assert_eq!(v.now.title, "Playlist 1");
        assert!(v.now.loading);
        assert!(!v.mini_visible, "mini player hides on Now Playing");
        c.handle(Input::Player(PlayerUpdate::TrackChanged(track(2))), 0);
        c.handle(
            Input::Player(PlayerUpdate::Playing {
                position_ms: 45_000,
            }),
            0,
        );
        let v = build(c.state());
        assert_eq!(v.now.title, "Song 2");
        assert!((v.now.progress - 0.25).abs() < 0.001);
        assert_eq!(v.now.position, "0:45");
        assert_eq!(v.now.duration, "3:00");
        assert!(v.now.playing);
        c.handle(ui(UiAction::Back), 0);
        assert!(build(c.state()).mini_visible);
    }

    #[test]
    fn detail_survives_while_now_playing_is_on_top() {
        let mut c = core();
        with_tracks(&mut c, "spotify:playlist:p1", 2);
        c.handle(
            ui(UiAction::OpenCollection("spotify:playlist:p1".into())),
            0,
        );
        c.handle(
            ui(UiAction::PlayCollection {
                uri: "spotify:playlist:p1".into(),
                shuffle: false,
            }),
            0,
        );
        let v = build(c.state());
        assert_eq!(
            v.detail.map(|d| d.header.uri),
            Some("spotify:playlist:p1".to_string())
        );
    }

    #[test]
    fn banner_prefers_notice_over_speaker_warning() {
        let mut c = core();
        c.handle(Input::Speaker { connected: false }, 0);
        assert_eq!(
            build(c.state()).banner.as_deref(),
            Some("Speaker not connected")
        );
        c.handle(Input::Player(PlayerUpdate::Unavailable), 0);
        assert_eq!(
            build(c.state()).banner.as_deref(),
            Some("That song can't play, skipping")
        );
        c.handle(Input::Player(PlayerUpdate::Disconnected), 0);
        c.handle(
            ui(UiAction::PlayCollection {
                uri: "spotify:playlist:p9".into(),
                shuffle: false,
            }),
            0,
        );
        assert_eq!(
            build(c.state()).banner.as_deref(),
            Some("No internet right now")
        );
    }
}
