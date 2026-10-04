use super::*;
use crate::model::{
    Account, Collection, CollectionKind, EditOutcome, LIKED_URI, PlaylistEdit, SearchResults,
    Section, Track,
};
use crate::settings::{Output, Speaker};

pub(crate) fn config() -> CoreConfig {
    CoreConfig {
        dim_after_ms: 60_000,
        off_after_ms: 120_000,
        initial_volume: 50,
        device_name: "Muzak Test".into(),
        speaker: None,
        saved: Default::default(),
        bluetooth: true,
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
        ..Default::default()
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
        album_uri: Some("spotify:album:a1".into()),
        artist_uri: Some("spotify:artist:r1".into()),
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
fn new_requests_initial_sections_and_shows_albums() {
    let (core, effects) = Core::new(config(), 0);
    assert_eq!(core.state().screen, Screen::Grid(Section::Albums));
    assert_eq!(
        effects,
        vec![
            Effect::Library(LibraryRequest::Section(Section::Playlists)),
            Effect::Library(LibraryRequest::Section(Section::Albums)),
            Effect::Library(LibraryRequest::Section(Section::Recent)),
            Effect::Library(LibraryRequest::Tracks {
                collection_uri: LIKED_URI.into()
            }),
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
    assert_eq!(c.state().screen, Screen::Grid(Section::Albums));
}

#[test]
fn back_on_empty_stack_is_noop() {
    let mut c = core();
    let fx = c.handle(ui(UiAction::Back), 0);
    assert!(fx.is_empty());
    assert_eq!(c.state().screen, Screen::Grid(Section::Albums));
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
        start_uri: None,
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
    assert_eq!(c.state().screen, Screen::Grid(Section::Albums));
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

// ---- Search and artists ----

fn searching() -> Core {
    let mut c = core();
    c.handle(ui(UiAction::ShowSection(Section::Search)), 0);
    c
}

fn type_text(c: &mut Core, text: &str, at_ms: u64) {
    c.handle(ui(UiAction::KeyPressed(text.into())), at_ms);
}

fn search_requests(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Library(LibraryRequest::Search(q)) => Some(q.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn search_section_opens_with_keyboard() {
    let c = searching();
    assert_eq!(c.state().screen, Screen::Search);
    assert_eq!(c.state().section, Section::Search);
    assert!(c.state().keyboard_open);
    assert!(c.wants_search_tick());
}

#[test]
fn search_waits_for_a_pause_in_typing_and_sends_once() {
    let mut c = searching();
    type_text(&mut c, "a", 1_000);
    type_text(&mut c, "b", 1_100);
    assert!(search_requests(&c.handle(Input::SearchTick, 1_499)).is_empty());
    assert_eq!(search_requests(&c.handle(Input::SearchTick, 1_500)), ["ab"]);
    assert!(search_requests(&c.handle(Input::SearchTick, 2_000)).is_empty());
    assert!(c.state().search.results.loading);
}

#[test]
fn query_is_trimmed_and_whitespace_only_sends_nothing() {
    let mut c = searching();
    type_text(&mut c, " ", 0);
    assert!(search_requests(&c.handle(Input::SearchTick, 1_000)).is_empty());
    type_text(&mut c, "abba ", 1_000);
    assert_eq!(
        search_requests(&c.handle(Input::SearchTick, 2_000)),
        ["abba"]
    );
}

#[test]
fn clearing_drops_results_without_a_request() {
    let mut c = searching();
    type_text(&mut c, "ab", 0);
    c.handle(Input::SearchTick, 1_000);
    let fx = c.handle(ui(UiAction::ClearSearch), 1_100);
    assert!(search_requests(&fx).is_empty());
    assert_eq!(c.state().search.query, "");
    assert_eq!(c.state().search.results, Slot::default());
}

#[test]
fn stale_results_are_dropped_and_current_ones_kept() {
    let mut c = searching();
    type_text(&mut c, "ab", 0);
    c.handle(Input::SearchTick, 1_000);
    let results = |name: &str| {
        Input::Library(LibraryUpdate::SearchResults {
            query: name.into(),
            results: SearchResults {
                playlists: vec![playlist(1)],
                ..Default::default()
            },
        })
    };
    c.handle(results("a"), 1_000);
    assert!(c.state().search.results.data.is_none());
    c.handle(results("ab"), 1_000);
    assert!(c.state().search.results.data.is_some());
    assert!(!c.state().search.results.loading);
}

#[test]
fn typing_is_ignored_off_the_search_screen() {
    let mut c = core();
    type_text(&mut c, "x", 0);
    assert_eq!(c.state().search.query, "");
}

#[test]
fn leaving_search_closes_the_keyboard() {
    let mut c = searching();
    c.handle(ui(UiAction::OpenCollection("spotify:album:a1".into())), 0);
    assert!(!c.state().keyboard_open);
    c.handle(ui(UiAction::Back), 0);
    assert_eq!(c.state().screen, Screen::Search);
}

#[test]
fn open_artist_requests_albums_and_back_returns_to_search() {
    let mut c = searching();
    let fx = c.handle(ui(UiAction::OpenArtist("spotify:artist:r1".into())), 0);
    assert_eq!(c.state().screen, Screen::Artist("spotify:artist:r1".into()));
    assert!(fx.contains(&Effect::Library(LibraryRequest::ArtistAlbums {
        artist_uri: "spotify:artist:r1".into()
    })));
    c.handle(ui(UiAction::Back), 0);
    assert_eq!(c.state().screen, Screen::Search);
}

#[test]
fn play_song_plays_it_within_its_album() {
    let mut c = searching();
    type_text(&mut c, "song", 0);
    c.handle(Input::SearchTick, 1_000);
    c.handle(
        Input::Library(LibraryUpdate::SearchResults {
            query: "song".into(),
            results: SearchResults {
                tracks: vec![track(7)],
                ..Default::default()
            },
        }),
        1_000,
    );
    let fx = c.handle(ui(UiAction::PlaySong("spotify:track:t7".into())), 2_000);
    assert!(fx.contains(&Effect::Player(PlayerCommand::Load {
        context_uri: "spotify:album:a1".into(),
        start_index: None,
        start_uri: Some("spotify:track:t7".into()),
        shuffle: false,
    })));
    assert_eq!(c.state().playback.track, Some(track(7)));
    assert_eq!(c.state().screen, Screen::NowPlaying);
}

#[test]
fn play_song_without_an_album_plays_the_track_itself() {
    let mut c = searching();
    let mut lonely = track(8);
    lonely.album_uri = None;
    c.state.search.results.data = Some(Arc::new(SearchResults {
        tracks: vec![lonely],
        ..Default::default()
    }));
    let fx = c.handle(ui(UiAction::PlaySong("spotify:track:t8".into())), 0);
    assert!(fx.iter().any(|e| matches!(
        e,
        Effect::Player(PlayerCommand::Load { context_uri, .. }) if context_uri == "spotify:track:t8"
    )));
}

#[test]
fn forbidden_track_list_is_marked_forbidden_not_failed() {
    let mut c = core();
    c.handle(
        Input::Library(LibraryUpdate::TracksFailed {
            collection_uri: "spotify:playlist:p9".into(),
            reason: FailReason::Forbidden,
        }),
        0,
    );
    let slot = &c.state().tracks["spotify:playlist:p9"];
    assert!(slot.forbidden && !slot.failed);
}

#[test]
fn closing_search_returns_to_the_previous_section() {
    let mut c = core();
    c.handle(ui(UiAction::ShowSection(Section::Albums)), 0);
    c.handle(ui(UiAction::ShowSection(Section::Search)), 0);
    c.handle(ui(UiAction::OpenArtist("spotify:artist:r1".into())), 0);
    c.handle(ui(UiAction::Back), 0);
    c.handle(ui(UiAction::CloseSearch), 0);
    assert_eq!(c.state().screen, Screen::Grid(Section::Albums));
    assert_eq!(c.state().section, Section::Albums);
    assert!(!c.state().keyboard_open);
}

// ---- Playlist editing ----

fn me() -> Account {
    Account {
        id: "me".into(),
        name: "Sam".into(),
    }
}

fn mine(n: u32) -> Collection {
    Collection {
        owner_id: Some("me".into()),
        snapshot_id: Some(format!("s{n}")),
        ..playlist(n)
    }
}

/// A core signed in as `me` with playlist 1 (owned, 3 songs) and playlist 2 (someone else's).
fn editor() -> Core {
    let mut c = core();
    c.handle(Input::Library(LibraryUpdate::Account(me())), 0);
    c.handle(
        Input::Library(LibraryUpdate::Section {
            section: Section::Playlists,
            items: vec![mine(1), playlist(2)],
        }),
        0,
    );
    with_tracks(&mut c, "spotify:playlist:p1", 3);
    c
}

fn edit_of(fx: &[Effect]) -> (u64, PlaylistEdit) {
    fx.iter()
        .find_map(|e| match e {
            Effect::Library(LibraryRequest::Edit { id, edit }) => Some((*id, edit.clone())),
            _ => None,
        })
        .expect("an edit")
}

fn track_names(c: &Core, uri: &str) -> Vec<String> {
    c.state().tracks[uri]
        .data
        .as_ref()
        .unwrap()
        .iter()
        .map(|t| t.name.clone())
        .collect()
}

#[test]
fn adding_a_song_shows_at_once_and_reloads_after_success() {
    let mut c = editor();
    c.state.search.results.data = Some(Arc::new(SearchResults {
        tracks: vec![track(9)],
        ..Default::default()
    }));
    c.handle(ui(UiAction::OpenPicker("spotify:track:t9".into())), 0);
    assert_eq!(c.state().picker.as_deref(), Some("spotify:track:t9"));
    let fx = c.handle(ui(UiAction::PickPlaylist("spotify:playlist:p1".into())), 0);
    let (id, edit) = edit_of(&fx);
    assert_eq!(
        edit,
        PlaylistEdit::Add {
            playlist_uri: "spotify:playlist:p1".into(),
            track_uri: "spotify:track:t9".into()
        }
    );
    assert_eq!(c.state().picker, None);
    assert_eq!(
        track_names(&c, "spotify:playlist:p1").last().unwrap(),
        "Song 9"
    );
    assert_eq!(c.state().notice, Some(Notice::AddedTo("Playlist 1".into())));
    let fx = c.handle(
        Input::Library(LibraryUpdate::EditDone {
            id,
            outcome: EditOutcome::Done,
        }),
        0,
    );
    assert!(fx.contains(&Effect::Library(LibraryRequest::ReloadTracks {
        collection_uri: "spotify:playlist:p1".into()
    })));
}

#[test]
fn a_refused_edit_is_undone_with_a_notice() {
    let mut c = editor();
    c.handle(
        ui(UiAction::OpenCollection("spotify:playlist:p1".into())),
        0,
    );
    c.handle(ui(UiAction::EditPlaylist("spotify:playlist:p1".into())), 0);
    let fx = c.handle(ui(UiAction::RemoveTrack(0)), 0);
    let (id, edit) = edit_of(&fx);
    assert_eq!(
        edit,
        PlaylistEdit::Remove {
            playlist_uri: "spotify:playlist:p1".into(),
            track_uri: "spotify:track:t0".into(),
            snapshot_id: Some("s1".into()),
        }
    );
    assert_eq!(track_names(&c, "spotify:playlist:p1"), ["Song 1", "Song 2"]);
    c.handle(
        Input::Library(LibraryUpdate::EditFailed {
            id,
            reason: FailReason::Forbidden,
        }),
        0,
    );
    assert_eq!(
        track_names(&c, "spotify:playlist:p1"),
        ["Song 0", "Song 1", "Song 2"]
    );
    assert_eq!(c.state().notice, Some(Notice::CouldntSave));
}

#[test]
fn duplicate_add_is_refused_with_a_notice() {
    let mut c = editor();
    c.handle(ui(UiAction::OpenPicker("spotify:track:t1".into())), 0);
    let fx = c.handle(ui(UiAction::PickPlaylist("spotify:playlist:p1".into())), 0);
    assert!(fx.is_empty());
    assert_eq!(
        c.state().notice,
        Some(Notice::AlreadyIn("Playlist 1".into()))
    );
}

#[test]
fn edits_are_refused_offline() {
    let mut c = editor();
    c.handle(Input::Player(PlayerUpdate::Disconnected), 0);
    c.handle(ui(UiAction::OpenPicker("spotify:track:t9".into())), 0);
    let fx = c.handle(ui(UiAction::PickPlaylist("spotify:playlist:p1".into())), 0);
    assert!(fx.is_empty());
    assert_eq!(c.state().notice, Some(Notice::NoInternet));
}

#[test]
fn only_owned_playlists_can_enter_edit_mode() {
    let mut c = editor();
    c.handle(
        ui(UiAction::OpenCollection("spotify:playlist:p2".into())),
        0,
    );
    c.handle(ui(UiAction::EditPlaylist("spotify:playlist:p2".into())), 0);
    assert_eq!(c.state().editing, None);
    c.handle(ui(UiAction::Back), 0);
    c.handle(
        ui(UiAction::OpenCollection("spotify:playlist:p1".into())),
        0,
    );
    c.handle(ui(UiAction::EditPlaylist("spotify:playlist:p1".into())), 0);
    assert_eq!(c.state().editing.as_deref(), Some("spotify:playlist:p1"));
    c.handle(ui(UiAction::Back), 0);
    assert_eq!(c.state().editing, None);
}

#[test]
fn creating_a_playlist_shows_a_placeholder_then_the_real_one() {
    let mut c = editor();
    c.handle(ui(UiAction::OpenPicker("spotify:track:t1".into())), 0);
    c.handle(ui(UiAction::NewPlaylist), 0);
    assert_eq!(c.state().picker, None);
    for key in ["R", "o", "a", "d"] {
        c.handle(ui(UiAction::KeyPressed(key.into())), 0);
    }
    c.handle(ui(UiAction::Backspace), 0);
    c.handle(ui(UiAction::KeyPressed("d".into())), 0);
    let fx = c.handle(ui(UiAction::KeyboardDone), 0);
    let (id, edit) = edit_of(&fx);
    assert_eq!(
        edit,
        PlaylistEdit::Create {
            name: "Road".into(),
            track_uri: "spotify:track:t1".into()
        }
    );
    assert_eq!(c.state().text_entry, None);
    let placeholder = c.state().sections[&Section::Playlists]
        .data
        .as_ref()
        .unwrap()[0]
        .clone();
    assert_eq!(placeholder.name, "Road");
    assert_eq!(track_names(&c, &placeholder.uri), ["Song 1"]);

    let real = mine(7);
    c.handle(
        Input::Library(LibraryUpdate::EditDone {
            id,
            outcome: EditOutcome::Created(real.clone()),
        }),
        0,
    );
    let lists = c.state().sections[&Section::Playlists]
        .data
        .clone()
        .unwrap();
    assert_eq!(lists[0], real);
    assert!(!c.state().tracks.contains_key(&placeholder.uri));
    assert_eq!(track_names(&c, &real.uri), ["Song 1"]);
}

#[test]
fn empty_names_are_not_saved() {
    let mut c = editor();
    c.handle(ui(UiAction::OpenPicker("spotify:track:t1".into())), 0);
    c.handle(ui(UiAction::NewPlaylist), 0);
    c.handle(ui(UiAction::KeyPressed("  ".into())), 0);
    let fx = c.handle(ui(UiAction::KeyboardDone), 0);
    assert!(fx.is_empty());
    assert!(c.state().text_entry.is_some());
    c.handle(ui(UiAction::CancelText), 0);
    assert_eq!(c.state().text_entry, None);
}

#[test]
fn rename_starts_with_the_current_name() {
    let mut c = editor();
    c.handle(
        ui(UiAction::OpenCollection("spotify:playlist:p1".into())),
        0,
    );
    c.handle(ui(UiAction::EditPlaylist("spotify:playlist:p1".into())), 0);
    c.handle(ui(UiAction::RenamePlaylist), 0);
    assert_eq!(c.state().text_entry.as_ref().unwrap().text, "Playlist 1");
    c.handle(ui(UiAction::KeyPressed("!".into())), 0);
    let fx = c.handle(ui(UiAction::KeyboardDone), 0);
    assert!(
        matches!(edit_of(&fx).1, PlaylistEdit::Rename { ref name, .. } if name == "Playlist 1!")
    );
    assert_eq!(
        c.state().sections[&Section::Playlists]
            .data
            .as_ref()
            .unwrap()[0]
            .name,
        "Playlist 1!"
    );
}

#[test]
fn move_reorders_and_sends_the_snapshot() {
    let mut c = editor();
    c.handle(
        ui(UiAction::OpenCollection("spotify:playlist:p1".into())),
        0,
    );
    c.handle(ui(UiAction::EditPlaylist("spotify:playlist:p1".into())), 0);
    let fx = c.handle(ui(UiAction::MoveTrack { from: 0, to: 2 }), 0);
    assert_eq!(
        edit_of(&fx).1,
        PlaylistEdit::Move {
            playlist_uri: "spotify:playlist:p1".into(),
            from: 0,
            to: 2,
            snapshot_id: Some("s1".into()),
        }
    );
    assert_eq!(
        track_names(&c, "spotify:playlist:p1"),
        ["Song 1", "Song 2", "Song 0"]
    );
    assert!(
        c.handle(ui(UiAction::MoveTrack { from: 0, to: 9 }), 0)
            .is_empty()
    );
}

#[test]
fn delete_asks_first_then_removes_and_returns_to_playlists() {
    let mut c = editor();
    c.handle(
        ui(UiAction::OpenCollection("spotify:playlist:p1".into())),
        0,
    );
    c.handle(ui(UiAction::EditPlaylist("spotify:playlist:p1".into())), 0);
    c.handle(ui(UiAction::AskDelete), 0);
    assert_eq!(
        c.state().confirm_delete.as_deref(),
        Some("spotify:playlist:p1")
    );
    c.handle(ui(UiAction::CancelDelete), 0);
    assert_eq!(c.state().confirm_delete, None);
    c.handle(ui(UiAction::AskDelete), 0);
    let fx = c.handle(ui(UiAction::ConfirmDelete), 0);
    assert!(matches!(edit_of(&fx).1, PlaylistEdit::Delete { .. }));
    assert_eq!(c.state().screen, Screen::Grid(Section::Playlists));
    assert_eq!(c.state().editing, None);
    let lists = c.state().sections[&Section::Playlists]
        .data
        .clone()
        .unwrap();
    assert!(lists.iter().all(|p| p.uri != "spotify:playlist:p1"));
}

#[test]
fn keys_go_to_the_name_field_not_search_while_it_is_open() {
    let mut c = editor();
    c.handle(ui(UiAction::ShowSection(Section::Search)), 0);
    c.handle(ui(UiAction::OpenPicker("spotify:track:t1".into())), 0);
    c.handle(ui(UiAction::NewPlaylist), 0);
    c.handle(ui(UiAction::KeyPressed("x".into())), 0);
    assert_eq!(c.state().search.query, "");
    assert_eq!(c.state().text_entry.as_ref().unwrap().text, "x");
}

#[test]
fn heart_likes_the_playing_song_and_undoes_on_failure() {
    let mut c = editor();
    with_tracks(&mut c, LIKED_URI, 2);
    // Liked Songs is loaded, so whether song 5 is liked is known without asking.
    let fx = c.handle(Input::Player(PlayerUpdate::TrackChanged(track(5))), 0);
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::Library(LibraryRequest::IsLiked { .. })))
    );
    c.handle(
        Input::Library(LibraryUpdate::Liked {
            track_uri: "spotify:track:t5".into(),
            liked: false,
        }),
        0,
    );
    let fx = c.handle(ui(UiAction::ToggleLike), 0);
    let (id, edit) = edit_of(&fx);
    assert_eq!(
        edit,
        PlaylistEdit::Like {
            track_uri: "spotify:track:t5".into()
        }
    );
    assert!(c.state().liked["spotify:track:t5"]);
    assert_eq!(track_names(&c, LIKED_URI)[0], "Song 5");
    c.handle(
        Input::Library(LibraryUpdate::EditFailed {
            id,
            reason: FailReason::Other,
        }),
        0,
    );
    assert!(!c.state().liked["spotify:track:t5"]);
    assert_eq!(track_names(&c, LIKED_URI), ["Song 0", "Song 1"]);
}

#[test]
fn list_view_toggles() {
    let mut c = core();
    assert!(!c.state().list_view);
    c.handle(ui(UiAction::ToggleListView), 0);
    assert!(c.state().list_view);
}

// ---- Settings ----

fn settings_effects(fx: &[Effect]) -> Vec<crate::settings::Settings> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::ApplySettings(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn found(address: &str, name: &str) -> Input {
    Input::Bluetooth(BtUpdate::Found(FoundSpeaker {
        address: address.into(),
        name: name.into(),
        paired: false,
    }))
}

#[test]
fn settings_opens_from_the_rail() {
    let mut c = core();
    c.handle(ui(UiAction::ShowSection(Section::Settings)), 0);
    assert_eq!(c.state().screen, Screen::Settings);
    assert_eq!(c.state().device.device_name, "Muzak Test");
}

#[test]
fn renaming_the_device_saves_and_restarts() {
    let mut c = core();
    c.handle(ui(UiAction::ShowSection(Section::Settings)), 0);
    c.handle(ui(UiAction::RenameDevice), 0);
    assert_eq!(c.state().text_entry.as_ref().unwrap().text, "Muzak Test");
    for _ in 0.."Muzak Test".len() {
        c.handle(ui(UiAction::Backspace), 0);
    }
    c.handle(ui(UiAction::KeyPressed("Kitchen".into())), 0);
    let fx = c.handle(ui(UiAction::KeyboardDone), 0);
    let saved = settings_effects(&fx);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].device_name.as_deref(), Some("Kitchen"));
    assert!(c.state().device.restarting);
}

#[test]
fn unchanged_device_name_is_not_saved() {
    let mut c = core();
    c.handle(ui(UiAction::RenameDevice), 0);
    let fx = c.handle(ui(UiAction::KeyboardDone), 0);
    assert!(settings_effects(&fx).is_empty());
    assert!(!c.state().device.restarting);
}

#[test]
fn scanning_then_connecting_saves_the_speaker() {
    let mut c = core();
    let fx = c.handle(ui(UiAction::FindSpeakers), 0);
    assert_eq!(fx, vec![Effect::Bluetooth(BtCommand::Scan)]);
    assert!(c.state().device.scanning);
    // A second tap while scanning does nothing.
    assert!(c.handle(ui(UiAction::FindSpeakers), 0).is_empty());
    c.handle(found("AA", "Boombox"), 0);
    c.handle(found("BB", "Kitchen"), 0);
    c.handle(found("AA", "Boombox 2"), 0);
    assert_eq!(c.state().device.found.len(), 2);
    c.handle(Input::Bluetooth(BtUpdate::ScanFinished), 0);
    assert!(!c.state().device.scanning);

    let fx = c.handle(ui(UiAction::ConnectSpeaker("AA".into())), 0);
    assert_eq!(fx, vec![Effect::Bluetooth(BtCommand::Connect("AA".into()))]);
    let fx = c.handle(Input::Bluetooth(BtUpdate::Connected("AA".into())), 0);
    assert_eq!(
        settings_effects(&fx)[0].output,
        Some(Output::Bluetooth(Speaker {
            address: "AA".into(),
            name: "Boombox 2".into()
        }))
    );
}

#[test]
fn a_failed_connection_names_the_speaker() {
    let mut c = core();
    c.handle(found("AA", "Boombox"), 0);
    c.handle(ui(UiAction::ConnectSpeaker("AA".into())), 0);
    let fx = c.handle(Input::Bluetooth(BtUpdate::ConnectFailed("AA".into())), 0);
    assert!(settings_effects(&fx).is_empty());
    assert_eq!(c.state().device.failed.as_deref(), Some("Boombox"));
    assert_eq!(c.state().device.connecting, None);
}

#[test]
fn forgetting_the_speaker_switches_to_the_jack() {
    let mut cfg = config();
    cfg.speaker = Some(Speaker {
        address: "AA".into(),
        name: "Boombox".into(),
    });
    let mut c = Core::new(cfg, 0).0;
    let fx = c.handle(ui(UiAction::ForgetSpeaker), 0);
    assert_eq!(fx, vec![Effect::Bluetooth(BtCommand::Forget("AA".into()))]);
    let fx = c.handle(Input::Bluetooth(BtUpdate::Forgotten("AA".into())), 0);
    assert_eq!(settings_effects(&fx)[0].output, Some(Output::Jack));
}

#[test]
fn no_bluetooth_means_no_scan() {
    let mut cfg = config();
    cfg.bluetooth = false;
    let mut c = Core::new(cfg, 0).0;
    assert!(c.handle(ui(UiAction::FindSpeakers), 0).is_empty());
}

// ---- Library saves ----

#[test]
fn liked_songs_in_the_add_sheet_likes_the_song() {
    let mut c = editor();
    with_tracks(&mut c, LIKED_URI, 2);
    c.state.search.results.data = Some(Arc::new(SearchResults {
        tracks: vec![track(9)],
        ..Default::default()
    }));
    c.handle(ui(UiAction::OpenPicker("spotify:track:t9".into())), 0);
    let fx = c.handle(ui(UiAction::PickLiked), 0);
    assert_eq!(
        edit_of(&fx).1,
        PlaylistEdit::Like {
            track_uri: "spotify:track:t9".into()
        }
    );
    assert_eq!(track_names(&c, LIKED_URI)[0], "Song 9");
    assert_eq!(
        c.state().notice,
        Some(Notice::AddedTo("Liked Songs".into()))
    );
    // Already liked: nothing to do.
    c.handle(ui(UiAction::OpenPicker("spotify:track:t0".into())), 0);
    assert!(c.handle(ui(UiAction::PickLiked), 0).is_empty());
    assert_eq!(
        c.state().notice,
        Some(Notice::AlreadyIn("Liked Songs".into()))
    );
}

#[test]
fn opening_an_album_asks_whether_it_is_saved_and_the_heart_saves_it() {
    let mut c = core();
    let album = Collection {
        uri: "spotify:album:x".into(),
        kind: CollectionKind::Album,
        name: "New Album".into(),
        ..Default::default()
    };
    c.state.search.results.data = Some(Arc::new(SearchResults {
        albums: vec![album.clone()],
        ..Default::default()
    }));
    let fx = c.handle(ui(UiAction::OpenCollection(album.uri.clone())), 0);
    assert!(fx.contains(&Effect::Library(LibraryRequest::IsLiked {
        track_uri: album.uri.clone()
    })));
    c.handle(
        Input::Library(LibraryUpdate::Liked {
            track_uri: album.uri.clone(),
            liked: false,
        }),
        0,
    );
    let fx = c.handle(ui(UiAction::ToggleSaveAlbum(album.uri.clone())), 0);
    let (id, edit) = edit_of(&fx);
    assert_eq!(
        edit,
        PlaylistEdit::SaveAlbum {
            album_uri: album.uri.clone()
        }
    );
    let albums = c.state().sections[&Section::Albums].data.clone().unwrap();
    assert_eq!(albums[0], album);
    // Spotify refuses: the album leaves the list again.
    c.handle(
        Input::Library(LibraryUpdate::EditFailed {
            id,
            reason: FailReason::Other,
        }),
        0,
    );
    assert!(!c.state().liked[&album.uri]);
    let albums = c
        .state()
        .sections
        .get(&Section::Albums)
        .and_then(|s| s.data.clone());
    assert!(albums.is_none_or(|a| a.iter().all(|x| x.uri != album.uri)));
}

// ---- Artists ----

#[test]
fn artists_drill_down_to_albums_and_back() {
    let mut c = core();
    c.handle(ui(UiAction::ShowSection(Section::Artists)), 0);
    assert_eq!(c.state().screen, Screen::Grid(Section::Artists));
    let fx = c.handle(ui(UiAction::OpenCollection("spotify:artist:r1".into())), 0);
    assert_eq!(c.state().screen, Screen::Artist("spotify:artist:r1".into()));
    assert!(fx.contains(&Effect::Library(LibraryRequest::ArtistAlbums {
        artist_uri: "spotify:artist:r1".into()
    })));
    c.handle(ui(UiAction::OpenCollection("spotify:album:a1".into())), 0);
    c.handle(ui(UiAction::Back), 0);
    assert_eq!(c.state().screen, Screen::Artist("spotify:artist:r1".into()));
    c.handle(ui(UiAction::Back), 0);
    assert_eq!(c.state().screen, Screen::Grid(Section::Artists));
}

#[test]
fn following_an_artist_adds_it_to_artists() {
    let mut c = core();
    let artist = Collection {
        uri: "spotify:artist:r1".into(),
        kind: CollectionKind::Artist,
        name: "Band".into(),
        ..Default::default()
    };
    c.state.search.results.data = Some(Arc::new(SearchResults {
        artists: vec![artist.clone()],
        ..Default::default()
    }));
    let fx = c.handle(ui(UiAction::ToggleFollow(artist.uri.clone())), 0);
    assert_eq!(
        edit_of(&fx).1,
        PlaylistEdit::Follow {
            artist_uri: artist.uri.clone()
        }
    );
    assert_eq!(
        c.state().sections[&Section::Artists].data.as_ref().unwrap()[0],
        artist
    );
    c.handle(ui(UiAction::ToggleFollow(artist.uri.clone())), 0);
    assert!(
        c.state().sections[&Section::Artists]
            .data
            .as_ref()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_playing_songs_artist_opens_their_page_with_a_name() {
    let mut c = core();
    c.handle(Input::Player(PlayerUpdate::TrackChanged(track(1))), 0);
    c.handle(ui(UiAction::OpenNowPlaying), 0);
    let fx = c.handle(ui(UiAction::OpenPlayingArtist), 0);
    assert_eq!(c.state().screen, Screen::Artist("spotify:artist:r1".into()));
    assert!(fx.contains(&Effect::Library(LibraryRequest::ArtistAlbums {
        artist_uri: "spotify:artist:r1".into()
    })));
    let v = crate::view::build(c.state());
    assert_eq!(v.artist.unwrap().header.title, "Band");
    c.handle(ui(UiAction::Back), 0);
    assert_eq!(c.state().screen, Screen::NowPlaying);
}

#[test]
fn album_artist_opens_the_artist_or_steps_back_to_them() {
    let mut c = core();
    let album = Collection {
        uri: "spotify:album:x".into(),
        kind: CollectionKind::Album,
        name: "Graceland".into(),
        subtitle: "Paul Simon".into(),
        artist_uri: Some("spotify:artist:ps".into()),
        ..Default::default()
    };
    c.handle(
        Input::Library(LibraryUpdate::Section {
            section: Section::Albums,
            items: vec![album.clone()],
        }),
        0,
    );
    // From Albums: opens the artist page, titled with their name.
    c.handle(ui(UiAction::OpenCollection(album.uri.clone())), 0);
    c.handle(ui(UiAction::OpenAlbumArtist(album.uri.clone())), 0);
    assert_eq!(c.state().screen, Screen::Artist("spotify:artist:ps".into()));
    assert_eq!(
        crate::view::build(c.state()).artist.unwrap().header.title,
        "Paul Simon"
    );
    // From that artist page into the album and back up: no second artist page.
    c.handle(ui(UiAction::OpenCollection(album.uri.clone())), 0);
    let depth = c.state().back_stack.len();
    c.handle(ui(UiAction::OpenAlbumArtist(album.uri.clone())), 0);
    assert_eq!(c.state().screen, Screen::Artist("spotify:artist:ps".into()));
    assert_eq!(c.state().back_stack.len(), depth - 1);
}

#[test]
fn an_album_cached_without_its_artist_finds_them_by_name() {
    let mut c = core();
    let album = Collection {
        uri: "spotify:album:old".into(),
        kind: CollectionKind::Album,
        name: "Graceland".into(),
        subtitle: "Paul Simon, Ladysmith".into(),
        ..Default::default()
    };
    c.handle(
        Input::Library(LibraryUpdate::Section {
            section: Section::Albums,
            items: vec![album.clone()],
        }),
        0,
    );
    c.handle(ui(UiAction::OpenCollection(album.uri.clone())), 0);
    let fx = c.handle(ui(UiAction::OpenAlbumArtist(album.uri.clone())), 0);
    assert!(fx.contains(&Effect::Library(LibraryRequest::FindArtist {
        album_uri: album.uri.clone(),
        name: "Paul Simon".into()
    })));
    let artist = Collection {
        uri: "spotify:artist:ps".into(),
        kind: CollectionKind::Artist,
        name: "Paul Simon".into(),
        ..Default::default()
    };
    c.handle(
        Input::Library(LibraryUpdate::ArtistFound {
            album_uri: album.uri.clone(),
            artist,
        }),
        0,
    );
    assert_eq!(c.state().screen, Screen::Artist("spotify:artist:ps".into()));
    let albums = c.state().sections[&Section::Albums].data.clone().unwrap();
    assert_eq!(albums[0].artist_uri.as_deref(), Some("spotify:artist:ps"));
}

// ---- Sleep timer ----

fn playing_at(volume: u8) -> Core {
    let mut c = core();
    c.handle(ui(UiAction::SetVolume { percent: volume }), 0);
    c.handle(Input::Player(PlayerUpdate::TrackChanged(track(1))), 0);
    c.handle(Input::Player(PlayerUpdate::Playing { position_ms: 0 }), 0);
    c
}

#[test]
fn sleep_timer_fades_then_pauses_and_restores_volume() {
    let mut c = playing_at(60);
    // The moon starts it straight away, for the default 30 minutes.
    c.handle(ui(UiAction::TapSleepTimer), 0);
    let ends = 30 * 60_000;
    assert_eq!(c.state().sleep_ends_ms, Some(ends));
    assert_eq!(crate::view::build(c.state()).now.sleep_left, "30 min");
    let player = |fx: Vec<Effect>| -> Vec<Effect> {
        fx.into_iter()
            .filter(|e| matches!(e, Effect::Player(_)))
            .collect()
    };
    // Halfway through the fade the volume is about half.
    assert!(!c.wants_search_tick());
    c.handle(Input::Tick, ends - 5_500);
    assert!(c.wants_search_tick(), "fast ticks for a smooth fade");
    let fx = player(c.handle(Input::SearchTick, ends - 2_500));
    assert_eq!(
        fx,
        vec![Effect::Player(PlayerCommand::SetVolume { percent: 30 })]
    );
    // At the end: pause, then the original volume comes back for next time.
    let fx = player(c.handle(Input::Tick, ends));
    assert_eq!(
        fx,
        vec![
            Effect::Player(PlayerCommand::Pause),
            Effect::Player(PlayerCommand::SetVolume { percent: 60 }),
        ]
    );
    assert_eq!(c.state().playback.status, PlayStatus::Paused);
    assert_eq!(c.state().playback.volume, 60);
    assert_eq!(c.state().sleep_ends_ms, None);
}

#[test]
fn tapping_the_moon_while_running_turns_it_off_and_restores_volume() {
    let mut c = playing_at(60);
    c.handle(ui(UiAction::SetSleepTimer(Some(30))), 0);
    c.handle(Input::Tick, 30 * 60_000 - 3_000);
    let fx = c.handle(ui(UiAction::TapSleepTimer), 30 * 60_000 - 2_000);

    assert!(fx.contains(&Effect::Player(PlayerCommand::SetVolume { percent: 60 })));
    assert_eq!(c.state().sleep_ends_ms, None);
}

#[test]
fn during_a_sleep_timer_the_screen_dims_in_seconds_then_goes_dark_after_a_minute() {
    let mut c = playing_at(60);
    c.handle(ui(UiAction::SetSleepTimer(Some(60))), 10_000);
    c.handle(Input::Tick, 12_000);
    assert_eq!(c.state().display, DisplayMode::Active);
    c.handle(Input::Tick, 13_000);
    assert_eq!(c.state().display, DisplayMode::Dim);
    // A new song doesn't light it up again.
    c.handle(
        Input::Player(PlayerUpdate::Playing { position_ms: 0 }),
        14_000,
    );
    assert_eq!(c.state().display, DisplayMode::Dim);
    c.handle(Input::Tick, 70_000);
    assert_eq!(c.state().display, DisplayMode::Off);
    // A touch wakes it, and the cycle starts again.
    c.handle(ui(UiAction::Touch), 80_000);
    assert_eq!(c.state().display, DisplayMode::Active);
    c.handle(Input::Tick, 83_000);
    assert_eq!(c.state().display, DisplayMode::Dim);
}

#[test]
fn the_sleep_length_comes_from_settings_and_saves_without_a_restart() {
    let mut c = playing_at(60);
    let fx = c.handle(ui(UiAction::SetSleepLength(60)), 0);
    assert!(matches!(&fx[..], [Effect::SaveSettings(s)] if s.sleep_minutes == Some(60)));
    assert!(!c.state().device.restarting);
    c.handle(ui(UiAction::TapSleepTimer), 1_000);
    assert_eq!(c.state().sleep_ends_ms, Some(1_000 + 60 * 60_000));
    c.handle(ui(UiAction::TapSleepTimer), 2_000);
    assert_eq!(c.state().sleep_ends_ms, None);
}
