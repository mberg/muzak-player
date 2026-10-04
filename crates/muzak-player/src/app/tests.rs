use super::*;
use crate::model::{Account, Collection, CollectionKind, LIKED_URI, Section, Track};

pub(crate) fn config() -> CoreConfig {
    CoreConfig {
        dim_after_ms: 60_000,
        off_after_ms: 120_000,
        initial_volume: 50,
    }
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
            Effect::Library(LibraryRequest::Account),
            Effect::Display(DisplayMode::Active),
        ]
    );
    assert!(core.state().sections[&Section::Playlists].loading);
    assert_eq!(core.state().playback.volume, 50);
}

#[test]
fn show_section_switches_grid_and_clears_back_stack() {
    let mut c = core();
    c.handle(
        ui(UiAction::OpenCollection("spotify:playlist:p1".into())),
        0,
    );
    let fx = c.handle(ui(UiAction::ShowSection(Section::Albums)), 0);
    assert_eq!(c.state().screen, Screen::Grid(Section::Albums));
    assert_eq!(c.state().section, Section::Albums);
    assert!(c.state().back_stack.is_empty());
    assert_eq!(
        fx,
        vec![Effect::Library(LibraryRequest::Section(Section::Albums))]
    );
}

#[test]
fn show_liked_opens_liked_detail() {
    let mut c = core();
    let fx = c.handle(ui(UiAction::ShowSection(Section::Liked)), 0);
    assert_eq!(c.state().screen, Screen::Detail(LIKED_URI.into()));
    assert_eq!(c.state().section, Section::Liked);
    assert_eq!(
        fx,
        vec![Effect::Library(LibraryRequest::Tracks {
            collection_uri: LIKED_URI.into()
        })]
    );
}

#[test]
fn open_collection_then_back() {
    let mut c = core();
    let fx = c.handle(
        ui(UiAction::OpenCollection("spotify:playlist:p1".into())),
        0,
    );
    assert_eq!(
        c.state().screen,
        Screen::Detail("spotify:playlist:p1".into())
    );
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
    c.handle(
        Input::Library(LibraryUpdate::SectionFailed {
            section: Section::Albums,
            reason: FailReason::Offline,
        }),
        0,
    );
    assert!(!c.state().online);
    c.handle(
        Input::Library(LibraryUpdate::Section {
            section: Section::Playlists,
            items: vec![playlist(1)],
        }),
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
        Input::Library(LibraryUpdate::Section {
            section: Section::Playlists,
            items: vec![playlist(1)],
        }),
        0,
    );
    c.handle(
        Input::Library(LibraryUpdate::SectionFailed {
            section: Section::Playlists,
            reason: FailReason::Other,
        }),
        0,
    );
    let slot = &c.state().sections[&Section::Playlists];
    assert!(slot.data.is_some());
    assert!(!slot.failed);

    c.handle(
        Input::Library(LibraryUpdate::SectionFailed {
            section: Section::Albums,
            reason: FailReason::Other,
        }),
        0,
    );
    assert!(c.state().sections[&Section::Albums].failed);
}

// Ruling R11: only the player's login rejection shows the sign-in screen.
#[test]
fn only_player_auth_invalid_sets_auth_needed() {
    let mut c = core();
    c.handle(
        Input::Library(LibraryUpdate::TracksFailed {
            collection_uri: "x".into(),
            reason: FailReason::Auth,
        }),
        0,
    );
    assert!(!c.state().auth_needed);
    assert!(c.state().tracks["x"].failed, "shows Can't load this");
    c.handle(
        Input::Library(LibraryUpdate::SectionFailed {
            section: Section::Albums,
            reason: FailReason::Auth,
        }),
        0,
    );
    assert!(!c.state().auth_needed);
    assert!(c.state().online);
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
}

fn load(uri: &str, start: Option<u32>, shuffle: bool) -> Effect {
    Effect::Player(PlayerCommand::Load {
        context_uri: uri.into(),
        start_index: start,
        shuffle,
    })
}

#[test]
fn play_collection_is_optimistic() {
    let mut c = core();
    with_tracks(&mut c, "spotify:playlist:p1", 3);
    c.handle(
        ui(UiAction::OpenCollection("spotify:playlist:p1".into())),
        0,
    );
    let fx = c.handle(
        ui(UiAction::PlayCollection {
            uri: "spotify:playlist:p1".into(),
            shuffle: false,
        }),
        0,
    );
    assert_eq!(fx, vec![load("spotify:playlist:p1", Some(0), false)]);
    let pb = &c.state().playback;
    assert_eq!(pb.status, PlayStatus::Loading);
    assert_eq!(pb.track, Some(track(0)));
    assert_eq!(pb.context_uri.as_deref(), Some("spotify:playlist:p1"));
    assert_eq!(c.state().screen, Screen::NowPlaying);
    c.handle(ui(UiAction::Back), 0);
    assert_eq!(
        c.state().screen,
        Screen::Detail("spotify:playlist:p1".into())
    );
}

#[test]
fn play_collection_shuffled_has_no_known_first_track() {
    let mut c = core();
    with_tracks(&mut c, "spotify:playlist:p1", 3);
    let fx = c.handle(
        ui(UiAction::PlayCollection {
            uri: "spotify:playlist:p1".into(),
            shuffle: true,
        }),
        0,
    );
    assert_eq!(fx, vec![load("spotify:playlist:p1", None, true)]);
    assert_eq!(c.state().playback.track, None);
    assert!(c.state().playback.shuffle);
}

// Review focus 1: double taps.
#[test]
fn double_tap_play_sends_one_load() {
    let mut c = core();
    let play = UiAction::PlayCollection {
        uri: "spotify:playlist:p1".into(),
        shuffle: false,
    };
    let first = c.handle(ui(play.clone()), 10_000);
    let second = c.handle(ui(play.clone()), 10_400);
    assert_eq!(first, vec![load("spotify:playlist:p1", Some(0), false)]);
    assert!(second.is_empty());
    assert_eq!(c.state().back_stack.len(), 1, "no duplicate navigation");
    let third = c.handle(ui(play), 11_500);
    assert_eq!(third, vec![load("spotify:playlist:p1", Some(0), false)]);
}

#[test]
fn play_track_uses_index_and_current_shuffle() {
    let mut c = core();
    with_tracks(&mut c, "spotify:album:a1", 5);
    c.handle(Input::Player(PlayerUpdate::Shuffle(true)), 0);
    let fx = c.handle(
        ui(UiAction::PlayTrack {
            collection_uri: "spotify:album:a1".into(),
            index: 3,
        }),
        0,
    );
    assert_eq!(fx, vec![load("spotify:album:a1", Some(3), true)]);
    assert_eq!(c.state().playback.track, Some(track(3)));
}

#[test]
fn toggle_play_flips_status_optimistically() {
    let mut c = core();
    assert!(
        c.handle(ui(UiAction::TogglePlay), 0).is_empty(),
        "nothing loaded"
    );
    c.handle(
        ui(UiAction::PlayCollection {
            uri: "spotify:playlist:p1".into(),
            shuffle: false,
        }),
        0,
    );
    c.handle(Input::Player(PlayerUpdate::Playing { position_ms: 0 }), 0);
    let fx = c.handle(ui(UiAction::TogglePlay), 0);
    assert_eq!(fx, vec![Effect::Player(PlayerCommand::Pause)]);
    assert_eq!(c.state().playback.status, PlayStatus::Paused);
    let fx = c.handle(ui(UiAction::TogglePlay), 0);
    assert_eq!(fx, vec![Effect::Player(PlayerCommand::Play)]);
    assert_eq!(c.state().playback.status, PlayStatus::Playing);
}

#[test]
fn player_updates_change_playback() {
    let mut c = core();
    c.handle(Input::Player(PlayerUpdate::TrackChanged(track(7))), 0);
    c.handle(
        Input::Player(PlayerUpdate::Playing { position_ms: 1_000 }),
        0,
    );
    c.handle(
        Input::Player(PlayerUpdate::Position { position_ms: 5_000 }),
        0,
    );
    c.handle(Input::Player(PlayerUpdate::Volume { percent: 120 }), 0);
    c.handle(
        Input::Player(PlayerUpdate::Repeat(crate::model::Repeat::Track)),
        0,
    );
    let pb = &c.state().playback;
    assert_eq!(pb.track, Some(track(7)));
    assert_eq!(pb.status, PlayStatus::Playing);
    assert_eq!(pb.position_ms, 5_000);
    assert_eq!(pb.volume, 100);
    assert_eq!(pb.repeat, crate::model::Repeat::Track);
    c.handle(
        Input::Player(PlayerUpdate::Paused { position_ms: 6_000 }),
        0,
    );
    assert_eq!(c.state().playback.status, PlayStatus::Paused);
    c.handle(Input::Player(PlayerUpdate::Stopped), 0);
    assert_eq!(c.state().playback.status, PlayStatus::Stopped);
}

#[test]
fn controls_emit_commands() {
    let mut c = core();
    c.handle(Input::Player(PlayerUpdate::TrackChanged(track(1))), 0);
    assert_eq!(
        c.handle(ui(UiAction::Next), 0),
        vec![Effect::Player(PlayerCommand::Next)]
    );
    assert_eq!(
        c.handle(ui(UiAction::Previous), 0),
        vec![Effect::Player(PlayerCommand::Previous)]
    );
    assert_eq!(
        c.handle(
            ui(UiAction::Seek {
                position_ms: 999_999
            }),
            0
        ),
        vec![Effect::Player(PlayerCommand::Seek {
            position_ms: 180_000
        })],
        "seek is clamped to the track length"
    );
    assert_eq!(
        c.handle(ui(UiAction::SetVolume { percent: 150 }), 0),
        vec![Effect::Player(PlayerCommand::SetVolume { percent: 100 })]
    );
    assert_eq!(
        c.handle(ui(UiAction::ToggleShuffle), 0),
        vec![Effect::Player(PlayerCommand::SetShuffle(true))]
    );
    assert_eq!(
        c.handle(ui(UiAction::CycleRepeat), 0),
        vec![Effect::Player(PlayerCommand::SetRepeat(
            crate::model::Repeat::Context
        ))]
    );
}

#[test]
fn open_now_playing_needs_something_playing() {
    let mut c = core();
    c.handle(ui(UiAction::OpenNowPlaying), 0);
    assert_eq!(c.state().screen, Screen::Grid(Section::Playlists));
    c.handle(Input::Player(PlayerUpdate::TrackChanged(track(1))), 0);
    c.handle(ui(UiAction::OpenNowPlaying), 0);
    assert_eq!(c.state().screen, Screen::NowPlaying);
}

#[test]
fn unavailable_shows_notice_that_expires() {
    let mut c = core();
    c.handle(Input::Player(PlayerUpdate::Unavailable), 1_000);
    assert_eq!(c.state().notice, Some(Notice::TrackUnavailable));
    c.handle(Input::Tick, 3_000);
    assert_eq!(c.state().notice, Some(Notice::TrackUnavailable));
    c.handle(Input::Tick, 5_100);
    assert_eq!(c.state().notice, None);
}

#[test]
fn play_while_offline_shows_notice_but_still_loads() {
    let mut c = core();
    c.handle(Input::Player(PlayerUpdate::Disconnected), 0);
    let fx = c.handle(
        ui(UiAction::PlayCollection {
            uri: "spotify:playlist:p1".into(),
            shuffle: false,
        }),
        0,
    );
    assert_eq!(fx, vec![load("spotify:playlist:p1", Some(0), false)]);
    assert_eq!(c.state().notice, Some(Notice::NoInternet));
}

#[test]
fn idle_dims_then_turns_off_and_touch_wakes() {
    let mut c = core();
    assert!(c.handle(Input::Tick, 59_000).is_empty());
    assert_eq!(
        c.handle(Input::Tick, 60_000),
        vec![Effect::Display(DisplayMode::Dim)]
    );
    assert_eq!(
        c.handle(Input::Tick, 120_000),
        vec![Effect::Display(DisplayMode::Off)]
    );
    assert_eq!(c.state().display, DisplayMode::Off);
    let fx = c.handle(ui(UiAction::Touch), 121_000);
    assert_eq!(fx, vec![Effect::Display(DisplayMode::Active)]);
    assert!(c.handle(Input::Tick, 122_000).is_empty());
}

#[test]
fn playing_keeps_screen_awake_and_wakes_it() {
    let mut c = core();
    c.handle(Input::Tick, 60_000);
    assert_eq!(c.state().display, DisplayMode::Dim);
    let fx = c.handle(
        Input::Player(PlayerUpdate::Playing { position_ms: 0 }),
        61_000,
    );
    assert_eq!(fx, vec![Effect::Display(DisplayMode::Active)]);
    assert!(c.handle(Input::Tick, 500_000).is_empty());
    assert_eq!(c.state().display, DisplayMode::Active);
}

// Review focus 5: Wi-Fi down at boot.
#[test]
fn reconnect_rerequests_library_and_open_detail() {
    let mut c = core();
    c.handle(
        Input::Library(LibraryUpdate::SectionFailed {
            section: Section::Playlists,
            reason: FailReason::Offline,
        }),
        0,
    );
    c.handle(ui(UiAction::OpenCollection("spotify:album:a1".into())), 0);
    let fx = c.handle(Input::Player(PlayerUpdate::Connected), 0);
    assert!(c.state().online);
    assert_eq!(
        fx,
        vec![
            Effect::Library(LibraryRequest::Section(Section::Playlists)),
            Effect::Library(LibraryRequest::Section(Section::Albums)),
            Effect::Library(LibraryRequest::Section(Section::Recent)),
            Effect::Library(LibraryRequest::Tracks {
                collection_uri: "spotify:album:a1".into()
            }),
            Effect::Library(LibraryRequest::Account),
        ]
    );
}

#[test]
fn account_is_stored_and_not_rerequested_on_reconnect() {
    let mut c = core();
    let account = Account {
        id: "1255644".into(),
        name: "Sam".into(),
    };
    c.handle(Input::Library(LibraryUpdate::Account(account.clone())), 0);
    assert_eq!(c.state().account, Some(account));
    let fx = c.handle(Input::Player(PlayerUpdate::Connected), 0);
    assert!(!fx.contains(&Effect::Library(LibraryRequest::Account)));
}

#[test]
fn disconnect_pauses_and_marks_offline() {
    let mut c = core();
    c.handle(Input::Player(PlayerUpdate::Playing { position_ms: 0 }), 0);
    c.handle(Input::Player(PlayerUpdate::Disconnected), 0);
    assert!(!c.state().online);
    assert_eq!(c.state().playback.status, PlayStatus::Paused);
}

#[test]
fn speaker_status_is_tracked() {
    let mut c = core();
    c.handle(Input::Speaker { connected: false }, 0);
    assert!(!c.state().speaker_connected);
}

#[test]
fn reconnect_reissues_a_pending_load() {
    let mut c = core();
    c.handle(Input::Player(PlayerUpdate::Disconnected), 0);
    c.handle(
        ui(UiAction::PlayCollection {
            uri: "spotify:playlist:p1".into(),
            shuffle: true,
        }),
        0,
    );
    assert_eq!(c.state().playback.status, PlayStatus::Loading);
    let fx = c.handle(Input::Player(PlayerUpdate::Connected), 5_000);
    assert!(fx.contains(&load("spotify:playlist:p1", None, true)));

    // Nothing pending once playback started.
    c.handle(
        Input::Player(PlayerUpdate::Playing { position_ms: 0 }),
        6_000,
    );
    c.handle(Input::Player(PlayerUpdate::Disconnected), 7_000);
    let fx = c.handle(Input::Player(PlayerUpdate::Connected), 8_000);
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::Player(PlayerCommand::Load { .. })))
    );
}
