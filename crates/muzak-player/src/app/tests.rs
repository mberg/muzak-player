use super::*;
use crate::model::{Account, Collection, CollectionKind, LIKED_URI, SearchResults, Section, Track};

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
