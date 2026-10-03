use std::sync::Arc;

use super::*;
use crate::model::{Collection, CollectionKind, LIKED_URI, Section, Track};

pub(crate) fn config() -> CoreConfig {
    CoreConfig { dim_after_ms: 60_000, off_after_ms: 120_000, initial_volume: 50 }
}

pub(crate) fn core() -> Core {
    Core::new(config(), 0).0
}

pub(crate) fn playlist(n: u32) -> Collection {
    Collection {
        uri: format!("spotify:playlist:p{n}"),
        kind: CollectionKind::Playlist,
        name: format!("Playlist {n}"),
        subtitle: "Mum".into(),
        image_url: Some(format!("https://i.scdn.co/image/p{n}")),
    }
}

pub(crate) fn track(n: u32) -> Track {
    Track {
        uri: format!("spotify:track:t{n}"),
        name: format!("Song {n}"),
        artists: "Band".into(),
        album: "Album".into(),
        image_url: None,
        duration_ms: 180_000,
    }
}

pub(crate) fn ui(action: UiAction) -> Input {
    Input::Ui(action)
}

pub(crate) fn with_tracks(core: &mut Core, uri: &str, n: u32) {
    core.handle(
        Input::Library(LibraryUpdate::Tracks {
            collection_uri: uri.into(),
            tracks: (0..n).map(track).collect(),
        }),
        0,
    );
}

#[test]
fn new_requests_initial_sections_and_shows_playlists() {
    let (core, effects) = Core::new(config(), 0);
    assert_eq!(core.state().screen, Screen::Grid(Section::Playlists));
    assert_eq!(
        effects,
        vec![
            Effect::Library(LibraryRequest::Section(Section::Playlists)),
            Effect::Library(LibraryRequest::Section(Section::Albums)),
            Effect::Library(LibraryRequest::Section(Section::Recent)),
            Effect::Display(DisplayMode::Active),
        ]
    );
    assert!(core.state().sections[&Section::Playlists].loading);
    assert_eq!(core.state().playback.volume, 50);
}

#[test]
fn show_section_switches_grid_and_clears_back_stack() {
    let mut c = core();
    c.handle(ui(UiAction::OpenCollection("spotify:playlist:p1".into())), 0);
    let fx = c.handle(ui(UiAction::ShowSection(Section::Albums)), 0);
    assert_eq!(c.state().screen, Screen::Grid(Section::Albums));
    assert_eq!(c.state().section, Section::Albums);
    assert!(c.state().back_stack.is_empty());
    assert_eq!(fx, vec![Effect::Library(LibraryRequest::Section(Section::Albums))]);
}

#[test]
fn show_liked_opens_liked_detail() {
    let mut c = core();
    let fx = c.handle(ui(UiAction::ShowSection(Section::Liked)), 0);
    assert_eq!(c.state().screen, Screen::Detail(LIKED_URI.into()));
    assert_eq!(c.state().section, Section::Liked);
    assert_eq!(
        fx,
        vec![Effect::Library(LibraryRequest::Tracks { collection_uri: LIKED_URI.into() })]
    );
}

#[test]
fn open_collection_then_back() {
    let mut c = core();
    let fx = c.handle(ui(UiAction::OpenCollection("spotify:playlist:p1".into())), 0);
    assert_eq!(c.state().screen, Screen::Detail("spotify:playlist:p1".into()));
    assert!(c.state().tracks["spotify:playlist:p1"].loading);
    assert_eq!(
        fx,
        vec![Effect::Library(LibraryRequest::Tracks {
            collection_uri: "spotify:playlist:p1".into()
        })]
    );
    c.handle(ui(UiAction::Back), 0);
    assert_eq!(c.state().screen, Screen::Grid(Section::Playlists));
}

#[test]
fn back_on_empty_stack_is_noop() {
    let mut c = core();
    let fx = c.handle(ui(UiAction::Back), 0);
    assert!(fx.is_empty());
    assert_eq!(c.state().screen, Screen::Grid(Section::Playlists));
}

#[test]
fn section_update_stores_data_and_marks_online() {
    let mut c = core();
    c.handle(Input::Library(LibraryUpdate::SectionFailed { section: Section::Albums, reason: FailReason::Offline }), 0);
    assert!(!c.state().online);
    c.handle(
        Input::Library(LibraryUpdate::Section { section: Section::Playlists, items: vec![playlist(1)] }),
        0,
    );
    let slot = &c.state().sections[&Section::Playlists];
    assert_eq!(slot.data.as_deref(), Some(&vec![playlist(1)]));
    assert!(!slot.loading && !slot.failed);
    assert!(c.state().online);
}

#[test]
fn failure_keeps_cached_data_and_only_marks_failed_without_data() {
    let mut c = core();
    c.handle(
        Input::Library(LibraryUpdate::Section { section: Section::Playlists, items: vec![playlist(1)] }),
        0,
    );
    c.handle(Input::Library(LibraryUpdate::SectionFailed { section: Section::Playlists, reason: FailReason::Other }), 0);
    let slot = &c.state().sections[&Section::Playlists];
    assert!(slot.data.is_some());
    assert!(!slot.failed);

    c.handle(Input::Library(LibraryUpdate::SectionFailed { section: Section::Albums, reason: FailReason::Other }), 0);
    assert!(c.state().sections[&Section::Albums].failed);
}

#[test]
fn auth_failure_sets_auth_needed() {
    let mut c = core();
    c.handle(
        Input::Library(LibraryUpdate::TracksFailed { collection_uri: "x".into(), reason: FailReason::Auth }),
        0,
    );
    assert!(c.state().auth_needed);
    let mut c = core();
    c.handle(Input::AuthInvalid, 0);
    assert!(c.state().auth_needed);
}

#[test]
fn tracks_update_stores_data() {
    let mut c = core();
    with_tracks(&mut c, "spotify:playlist:p1", 3);
    let slot = &c.state().tracks["spotify:playlist:p1"];
    assert_eq!(slot.data.as_ref().map(|t| t.len()), Some(3));
    assert!(Arc::strong_count(slot.data.as_ref().unwrap()) >= 1);
}
