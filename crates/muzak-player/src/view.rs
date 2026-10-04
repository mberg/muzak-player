//! Turns `AppState` into plain display data. No Slint types here, so it is unit-testable.

use std::sync::Arc;

use crate::app::{AppState, DisplayMode, Notice, PlayStatus, Screen, SearchFilter, Slot};
use crate::library::matching::matches;
use crate::model::{Collection, LIKED_URI, Repeat, Section, Track, liked_collection};

/// Library matches shown per kind before the catalog results.
const LIBRARY_MATCHES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenView {
    Grid,
    Detail,
    NowPlaying,
    Search,
    Artist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadStatus {
    Loading,
    Empty,
    Failed,
    /// Spotify won't list it for this app, though playing it works.
    Forbidden,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Header,
    Song,
    Album,
    Artist,
    Playlist,
}

/// One line of search results: a group header or a result.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchRowView {
    pub kind: RowKind,
    pub title: String,
    pub subtitle: String,
    pub image_url: Option<String>,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchView {
    pub query: String,
    pub keyboard: bool,
    pub rows: Vec<SearchRowView>,
    pub status: LoadStatus,
    /// Shown under library matches when the Spotify search could not run.
    pub note: String,
    pub filter: SearchFilter,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArtistView {
    pub header: TileView,
    pub albums: Vec<TileView>,
    pub status: LoadStatus,
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
    /// Small label in the rail naming the signed-in account; empty until known.
    pub account: String,
    pub search: SearchView,
    pub artist: Option<ArtistView>,
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
        Screen::Search => ScreenView::Search,
        Screen::Artist(_) => ScreenView::Artist,
    };
    let grid_section = match state.screen {
        Screen::Grid(section) => section,
        _ if matches!(state.section, Section::Liked | Section::Search) => Section::Playlists,
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
        mini_visible: has_playback && screen != ScreenView::NowPlaying && !state.keyboard_open,
        banner: banner(state),
        display: state.display,
        auth_needed: state.auth_needed,
        account: state
            .account
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_default(),
        search: search(state),
        artist: artist(state),
    }
}

fn row(kind: RowKind, c: &Collection) -> SearchRowView {
    SearchRowView {
        kind,
        title: c.name.clone(),
        subtitle: c.subtitle.clone(),
        image_url: c.image_url.clone(),
        uri: c.uri.clone(),
    }
}

fn song_row(t: &Track) -> SearchRowView {
    SearchRowView {
        kind: RowKind::Song,
        title: t.name.clone(),
        subtitle: t.artists.clone(),
        image_url: t.image_url.clone(),
        uri: t.uri.clone(),
    }
}

fn header_row(title: &str) -> SearchRowView {
    SearchRowView {
        kind: RowKind::Header,
        title: title.into(),
        subtitle: String::new(),
        image_url: None,
        uri: String::new(),
    }
}

fn section_items(state: &AppState, section: Section) -> &[Collection] {
    state
        .sections
        .get(&section)
        .and_then(|slot| slot.data.as_deref())
        .map_or(&[], Vec::as_slice)
}

/// Matches from the user's own playlists, saved albums and Liked Songs.
fn library_rows(state: &AppState, query: &str) -> Vec<SearchRowView> {
    let mut rows = Vec::new();
    let liked = state
        .tracks
        .get(LIKED_URI)
        .and_then(|slot| slot.data.as_deref())
        .map_or(&[][..], Vec::as_slice);
    rows.extend(
        liked
            .iter()
            .filter(|t| matches(&format!("{} {}", t.name, t.artists), query))
            .take(LIBRARY_MATCHES)
            .map(song_row),
    );
    rows.extend(
        section_items(state, Section::Albums)
            .iter()
            .filter(|c| matches(&format!("{} {}", c.name, c.subtitle), query))
            .take(LIBRARY_MATCHES)
            .map(|c| row(RowKind::Album, c)),
    );
    rows.extend(
        section_items(state, Section::Playlists)
            .iter()
            .filter(|c| matches(&c.name, query))
            .take(LIBRARY_MATCHES)
            .map(|c| row(RowKind::Playlist, c)),
    );
    rows
}

/// Whether a result row of `kind` passes the filter.
fn shows(filter: SearchFilter, kind: RowKind) -> bool {
    matches!(
        (filter, kind),
        (SearchFilter::All, _)
            | (SearchFilter::Songs, RowKind::Song)
            | (SearchFilter::Artists, RowKind::Artist)
            | (SearchFilter::Albums, RowKind::Album)
            | (SearchFilter::Playlists, RowKind::Playlist)
    )
}

fn search(state: &AppState) -> SearchView {
    let s = &state.search;
    let query = s.query.trim();
    let mut rows = Vec::new();
    let library: Vec<SearchRowView> = library_rows(state, query)
        .into_iter()
        .filter(|r| shows(s.filter, r.kind))
        .collect();
    let has_library = !library.is_empty();
    if has_library {
        rows.push(header_row("In your library"));
        rows.extend(library);
    }
    // Catalog results count only when they belong to what is typed now.
    let current = !query.is_empty() && s.sent.trim() == query;
    let catalog = s.results.data.as_ref().filter(|_| current);
    if let Some(found) = catalog {
        let shown: std::collections::HashSet<String> = rows.iter().map(|r| r.uri.clone()).collect();
        let fresh = |r: &SearchRowView| !shown.contains(&r.uri);
        let catalog_rows: Vec<SearchRowView> = found
            .tracks
            .iter()
            .map(song_row)
            .chain(found.artists.iter().map(|c| row(RowKind::Artist, c)))
            .chain(found.albums.iter().map(|c| row(RowKind::Album, c)))
            .chain(found.playlists.iter().map(|c| row(RowKind::Playlist, c)))
            .filter(fresh)
            .filter(|r| shows(s.filter, r.kind))
            .collect();
        if !catalog_rows.is_empty() {
            rows.push(header_row("On Spotify"));
            rows.extend(catalog_rows);
        }
    }
    let catalog_failed = current && s.results.failed;
    let status = if query.is_empty() || !rows.is_empty() {
        LoadStatus::Ready
    } else if catalog_failed {
        LoadStatus::Failed
    } else if s.results.loading || !current {
        LoadStatus::Loading
    } else {
        LoadStatus::Empty
    };
    let note = match (catalog_failed && has_library, state.online) {
        (false, _) => String::new(),
        (true, true) => "Can't search Spotify right now".into(),
        (true, false) => "No internet right now".into(),
    };
    SearchView {
        query: s.query.clone(),
        keyboard: state.keyboard_open,
        rows,
        status,
        note,
        filter: s.filter,
    }
}

fn artist(state: &AppState) -> Option<ArtistView> {
    let uri = std::iter::once(&state.screen)
        .chain(state.back_stack.iter().rev())
        .find_map(|screen| match screen {
            Screen::Artist(uri) => Some(uri.clone()),
            _ => None,
        })?;
    let header = find_collection(state, &uri)
        .map(|c| tile(&c))
        .unwrap_or_else(|| blank_tile(&uri));
    let slot = state.artist_albums.get(&uri);
    let albums = slot
        .and_then(|s| s.data.as_ref())
        .map(|albums| albums.iter().map(tile).collect())
        .unwrap_or_default();
    Some(ArtistView {
        header,
        albums,
        status: status(slot),
    })
}

fn blank_tile(uri: &str) -> TileView {
    TileView {
        uri: uri.to_string(),
        title: String::new(),
        subtitle: String::new(),
        image_url: None,
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
        Some(Slot {
            forbidden: true, ..
        }) => LoadStatus::Forbidden,
        Some(Slot { failed: true, .. }) => LoadStatus::Failed,
        _ => LoadStatus::Loading,
    }
}

fn find_collection(state: &AppState, uri: &str) -> Option<Collection> {
    if uri == LIKED_URI {
        return Some(liked_collection());
    }
    let from_sections = state
        .sections
        .values()
        .chain(state.artist_albums.values())
        .filter_map(|slot| slot.data.as_ref())
        .flat_map(|items: &Arc<Vec<Collection>>| items.iter());
    let from_search = state.search.results.data.iter().flat_map(|found| {
        found
            .albums
            .iter()
            .chain(&found.playlists)
            .chain(&found.artists)
    });
    from_sections
        .chain(from_search)
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
        .unwrap_or_else(|| blank_tile(&uri));
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
    match &state.notice {
        Some(Notice::NoInternet) => Some("No internet right now".into()),
        Some(Notice::TrackUnavailable) => Some("That song can't play, skipping".into()),
        Some(Notice::CouldntSave) => Some("Couldn't save that".into()),
        Some(Notice::AddedTo(name)) => Some(format!("Added to {name}")),
        Some(Notice::AlreadyIn(name)) => Some(format!("Already in {name}")),
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

    // ---- Search ----

    fn search_for(c: &mut crate::app::Core, query: &str) {
        c.handle(ui(UiAction::ShowSection(Section::Search)), 0);
        c.handle(ui(UiAction::KeyPressed(query.into())), 0);
        c.handle(Input::SearchTick, 1_000);
    }

    fn catalog(c: &mut crate::app::Core, query: &str, results: crate::model::SearchResults) {
        c.handle(
            Input::Library(LibraryUpdate::SearchResults {
                query: query.into(),
                results,
            }),
            1_000,
        );
    }

    fn kinds(v: &View) -> Vec<(RowKind, String)> {
        v.search
            .rows
            .iter()
            .map(|r| (r.kind, r.title.clone()))
            .collect()
    }

    #[test]
    fn library_matches_come_first_and_are_not_repeated() {
        let mut c = core();
        c.handle(
            Input::Library(LibraryUpdate::Section {
                section: Section::Playlists,
                items: vec![playlist(1), playlist(2)],
            }),
            0,
        );
        search_for(&mut c, "playlist 1");
        catalog(
            &mut c,
            "playlist 1",
            crate::model::SearchResults {
                playlists: vec![playlist(1), playlist(3)],
                ..Default::default()
            },
        );
        let v = build(c.state());
        assert_eq!(
            kinds(&v),
            [
                (RowKind::Header, "In your library".to_string()),
                (RowKind::Playlist, "Playlist 1".to_string()),
                (RowKind::Header, "On Spotify".to_string()),
                (RowKind::Playlist, "Playlist 3".to_string()),
            ]
        );
        assert_eq!(v.search.status, LoadStatus::Ready);
    }

    #[test]
    fn filter_keeps_only_one_kind_and_its_headers() {
        let mut c = core();
        c.handle(
            Input::Library(LibraryUpdate::Section {
                section: Section::Playlists,
                items: vec![playlist(1)],
            }),
            0,
        );
        search_for(&mut c, "1");
        catalog(
            &mut c,
            "1",
            crate::model::SearchResults {
                tracks: vec![track(1)],
                playlists: vec![playlist(9)],
                ..Default::default()
            },
        );
        c.handle(ui(UiAction::SetSearchFilter(SearchFilter::Songs)), 1_000);
        assert_eq!(
            kinds(&build(c.state())),
            [
                (RowKind::Header, "On Spotify".to_string()),
                (RowKind::Song, "Song 1".to_string()),
            ]
        );
        c.handle(ui(UiAction::SetSearchFilter(SearchFilter::Artists)), 1_000);
        assert_eq!(build(c.state()).search.status, LoadStatus::Empty);
    }

    #[test]
    fn liked_songs_match_on_title_or_artist() {
        let mut c = core();
        with_tracks(&mut c, LIKED_URI, 3);
        search_for(&mut c, "band song 2");
        let v = build(c.state());
        assert_eq!(v.search.rows[1].kind, RowKind::Song);
        assert_eq!(v.search.rows[1].title, "Song 2");
    }

    #[test]
    fn search_status_is_loading_then_empty() {
        let mut c = core();
        search_for(&mut c, "zzz");
        assert_eq!(build(c.state()).search.status, LoadStatus::Loading);
        catalog(&mut c, "zzz", Default::default());
        assert_eq!(build(c.state()).search.status, LoadStatus::Empty);
    }

    #[test]
    fn empty_query_is_ready_with_no_rows() {
        let mut c = core();
        c.handle(ui(UiAction::ShowSection(Section::Search)), 0);
        let v = build(c.state());
        assert_eq!(v.screen, ScreenView::Search);
        assert!(v.search.rows.is_empty());
        assert_eq!(v.search.status, LoadStatus::Ready);
        assert!(v.search.keyboard);
    }

    #[test]
    fn failed_search_keeps_library_rows_with_a_note() {
        let mut c = core();
        c.handle(
            Input::Library(LibraryUpdate::Section {
                section: Section::Playlists,
                items: vec![playlist(1)],
            }),
            0,
        );
        search_for(&mut c, "playlist");
        c.handle(
            Input::Library(LibraryUpdate::SearchFailed {
                query: "playlist".into(),
                reason: FailReason::Offline,
            }),
            1_000,
        );
        let v = build(c.state());
        assert_eq!(v.search.status, LoadStatus::Ready);
        assert_eq!(v.search.note, "No internet right now");
    }

    #[test]
    fn detail_header_comes_from_search_results() {
        let mut c = core();
        search_for(&mut c, "p");
        catalog(
            &mut c,
            "p",
            crate::model::SearchResults {
                playlists: vec![playlist(5)],
                ..Default::default()
            },
        );
        c.handle(ui(UiAction::OpenCollection(playlist(5).uri)), 1_000);
        assert_eq!(build(c.state()).detail.unwrap().header.title, "Playlist 5");
    }

    #[test]
    fn forbidden_detail_has_its_own_status() {
        let mut c = core();
        c.handle(
            ui(UiAction::OpenCollection("spotify:playlist:p9".into())),
            0,
        );
        c.handle(
            Input::Library(LibraryUpdate::TracksFailed {
                collection_uri: "spotify:playlist:p9".into(),
                reason: FailReason::Forbidden,
            }),
            0,
        );
        assert_eq!(
            build(c.state()).detail.unwrap().status,
            LoadStatus::Forbidden
        );
    }

    #[test]
    fn keyboard_hides_the_mini_player() {
        let mut c = core();
        c.handle(
            ui(UiAction::PlayCollection {
                uri: "spotify:album:a1".into(),
                shuffle: false,
            }),
            0,
        );
        c.handle(ui(UiAction::ShowSection(Section::Search)), 5_000);
        assert!(!build(c.state()).mini_visible);
        c.handle(ui(UiAction::CloseKeyboard), 5_000);
        assert!(build(c.state()).mini_visible);
    }
}
