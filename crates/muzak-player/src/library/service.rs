//! Serves library requests: cached data first, then a fresh fetch.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::{self, UnboundedSender};

use super::LibrarySource;
use super::cache::{DiskCache, tracks_key};
use crate::app::{Input, LibraryRequest, LibraryUpdate};
use crate::model::{Account, Collection, LIKED_URI, Section, Track};

const ACCOUNT_KEY: &str = "account";
/// A cached section or track list younger than this is not fetched again, except after
/// an edit. Spotify rate-limits developer apps hard, and library data rarely changes.
const CACHE_FRESH: std::time::Duration = std::time::Duration::from_secs(600);
/// Liked Songs is reused this long; likes made here update it locally anyway.
const LIKED_FRESH: std::time::Duration = std::time::Duration::from_secs(30 * 60);
/// An artist's album list is reused this long.
const ARTIST_FRESH: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

fn version_key(uri: &str) -> String {
    format!("version-{uri}")
}

/// The playlist's current version, from the cached playlist list.
fn listed_snapshot(cache: &DiskCache, uri: &str) -> Option<String> {
    cache
        .read::<Vec<Collection>>(Section::Playlists.cache_key())?
        .into_iter()
        .find(|p| p.uri == uri)?
        .snapshot_id
}

/// Whether a cached track list can be shown without asking Spotify:
/// albums never change, playlists change only when their version does, and Liked Songs
/// and anything else are reused for a while.
fn tracks_current(cache: &DiskCache, key: &str, uri: &str) -> bool {
    let age = cache.age(key);
    if uri.starts_with("spotify:album:") {
        return age.is_some();
    }
    if uri == LIKED_URI {
        return age.is_some_and(|a| a < LIKED_FRESH);
    }
    if uri.starts_with("spotify:playlist:")
        && let Some(listed) = listed_snapshot(cache, uri)
    {
        return cache.read::<String>(&version_key(uri)).as_deref() == Some(listed.as_str());
    }
    age.is_some_and(|a| a < CACHE_FRESH)
}

pub fn spawn_library<S: LibrarySource>(
    source: Arc<S>,
    cache: Arc<DiskCache>,
    inputs: UnboundedSender<Input>,
) -> UnboundedSender<LibraryRequest> {
    let (tx, mut rx) = mpsc::unbounded_channel::<LibraryRequest>();
    let in_flight = Arc::new(Mutex::new(HashSet::<LibraryRequest>::new()));
    tokio::spawn(async move {
        while let Some(request) = rx.recv().await {
            // Two identical adds are two adds, so edits are never de-duplicated.
            let is_edit = matches!(request, LibraryRequest::Edit { .. });
            if !is_edit && !in_flight.lock().unwrap().insert(request.clone()) {
                continue;
            }
            let (source, cache, inputs, in_flight) = (
                source.clone(),
                cache.clone(),
                inputs.clone(),
                in_flight.clone(),
            );
            tokio::spawn(async move {
                serve(&*source, &cache, &inputs, request.clone()).await;
                in_flight.lock().unwrap().remove(&request);
            });
        }
    });
    tx
}

async fn serve<S: LibrarySource>(
    source: &S,
    cache: &DiskCache,
    inputs: &UnboundedSender<Input>,
    request: LibraryRequest,
) {
    let send = |update| {
        let _ = inputs.send(Input::Library(update));
    };
    let force = matches!(
        request,
        LibraryRequest::Reload(_) | LibraryRequest::ReloadTracks { .. }
    );
    match request {
        LibraryRequest::Section(section) | LibraryRequest::Reload(section) => {
            let key = section.cache_key();
            if let Some(items) = cache.read::<Vec<Collection>>(key) {
                send(LibraryUpdate::Section { section, items });
                // A restart soon after a fetch (Settings restarts the player) reuses the
                // cache rather than asking Spotify again, which rate-limits heavily.
                if !force && cache.age(key).is_some_and(|age| age < CACHE_FRESH) {
                    return;
                }
            }
            match source.section(section).await {
                Ok(items) => {
                    if let Err(e) = cache.write(key, &items) {
                        tracing::warn!("cache write failed for {key}: {e}");
                    }
                    send(LibraryUpdate::Section { section, items });
                }
                Err(e) => {
                    tracing::warn!("loading {section:?} failed: {e}");
                    send(LibraryUpdate::SectionFailed {
                        section,
                        reason: e.reason(),
                    });
                }
            }
        }
        LibraryRequest::Tracks { collection_uri }
        | LibraryRequest::ReloadTracks { collection_uri } => {
            let key = tracks_key(&collection_uri);
            if let Some(tracks) = cache.read::<Vec<Track>>(&key) {
                send(LibraryUpdate::Tracks {
                    collection_uri: collection_uri.clone(),
                    tracks,
                });
                if !force && tracks_current(cache, &key, &collection_uri) {
                    return;
                }
            }
            match source.tracks(&collection_uri).await {
                Ok(tracks) => {
                    if let Err(e) = cache.write(&key, &tracks) {
                        tracing::warn!("cache write failed for {key}: {e}");
                    }
                    // Remember which version of the playlist this is.
                    if let Some(snapshot) = listed_snapshot(cache, &collection_uri) {
                        let _ = cache.write(&version_key(&collection_uri), &snapshot);
                    }
                    send(LibraryUpdate::Tracks {
                        collection_uri,
                        tracks,
                    });
                }
                Err(e) => {
                    tracing::warn!("loading {collection_uri} failed: {e}");
                    send(LibraryUpdate::TracksFailed {
                        collection_uri,
                        reason: e.reason(),
                    });
                }
            }
        }
        LibraryRequest::Search(query) => match source.search(&query).await {
            // Search results are not cached: they go stale quickly and are cheap to refetch.
            Ok(results) => send(LibraryUpdate::SearchResults { query, results }),
            Err(e) => {
                tracing::warn!("search for {query:?} failed: {e}");
                send(LibraryUpdate::SearchFailed {
                    query,
                    reason: e.reason(),
                });
            }
        },
        LibraryRequest::ArtistAlbums { artist_uri } => {
            let key = tracks_key(&artist_uri);
            if let Some(albums) = cache.read::<Vec<Collection>>(&key) {
                send(LibraryUpdate::ArtistAlbums {
                    artist_uri: artist_uri.clone(),
                    albums,
                });
                if cache.age(&key).is_some_and(|age| age < ARTIST_FRESH) {
                    return;
                }
            }
            match source.artist_albums(&artist_uri).await {
                Ok(albums) => {
                    if let Err(e) = cache.write(&key, &albums) {
                        tracing::warn!("cache write failed for {key}: {e}");
                    }
                    send(LibraryUpdate::ArtistAlbums { artist_uri, albums });
                }
                Err(e) => {
                    tracing::warn!("loading albums for {artist_uri} failed: {e}");
                    send(LibraryUpdate::ArtistAlbumsFailed {
                        artist_uri,
                        reason: e.reason(),
                    });
                }
            }
        }
        LibraryRequest::Edit { id, edit } => {
            let label = format!("{edit:?}");
            match source.apply(edit).await {
                Ok(outcome) => send(LibraryUpdate::EditDone { id, outcome }),
                Err(e) => {
                    tracing::warn!("playlist edit {label} failed: {e}");
                    send(LibraryUpdate::EditFailed {
                        id,
                        reason: e.reason(),
                    });
                }
            }
        }
        LibraryRequest::FindArtist { album_uri, name } => match source.find_artist(&name).await {
            Ok(Some(artist)) => send(LibraryUpdate::ArtistFound { album_uri, artist }),
            Ok(None) => tracing::warn!("no artist found named {name:?}"),
            Err(e) => tracing::warn!("finding artist {name:?} failed: {e}"),
        },
        LibraryRequest::IsLiked { track_uri } => match source.is_liked(&track_uri).await {
            Ok(liked) => send(LibraryUpdate::Liked { track_uri, liked }),
            // Only the heart depends on this; it stays unknown.
            Err(e) => tracing::warn!("checking whether {track_uri} is liked failed: {e}"),
        },
        LibraryRequest::Account => {
            if let Some(account) = cache.read::<Account>(ACCOUNT_KEY) {
                send(LibraryUpdate::Account(account));
            }
            match source.account().await {
                Ok(account) => {
                    if let Err(e) = cache.write(ACCOUNT_KEY, &account) {
                        tracing::warn!("cache write failed for {ACCOUNT_KEY}: {e}");
                    }
                    tracing::info!("library account: {} ({})", account.name, account.id);
                    send(LibraryUpdate::Account(account));
                }
                // Only the rail label depends on this, so a failure is just logged.
                Err(e) => tracing::warn!("loading the account failed: {e}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use super::*;
    use crate::app::FailReason;
    use crate::library::FetchError;
    use crate::model::{Collection, CollectionKind, SearchResults, Section, Track};

    struct StubSource {
        calls: AtomicU32,
        fail: bool,
    }

    fn collection(name: &str) -> Collection {
        Collection {
            uri: format!("spotify:playlist:{name}"),
            kind: CollectionKind::Playlist,
            name: name.into(),
            subtitle: String::new(),
            image_url: None,
            ..Default::default()
        }
    }

    impl LibrarySource for StubSource {
        async fn section(&self, _section: Section) -> Result<Vec<Collection>, FetchError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            if self.fail {
                Err(FetchError::Offline)
            } else {
                Ok(vec![collection("fresh")])
            }
        }
        async fn tracks(&self, _uri: &str) -> Result<Vec<Track>, FetchError> {
            Ok(vec![])
        }
        async fn search(&self, query: &str) -> Result<SearchResults, FetchError> {
            Ok(SearchResults {
                playlists: vec![collection(query)],
                ..Default::default()
            })
        }
    }

    #[tokio::test]
    async fn search_answers_with_its_query_and_is_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(DiskCache::new(dir.path()).unwrap());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let source = Arc::new(StubSource {
            calls: AtomicU32::new(0),
            fail: false,
        });
        let requests = spawn_library(source, cache, tx);
        requests
            .send(LibraryRequest::Search("abba".into()))
            .unwrap();
        match next_update(&mut rx).await {
            LibraryUpdate::SearchResults { query, results } => {
                assert_eq!(query, "abba");
                assert_eq!(results.playlists[0].name, "abba");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    async fn next_update(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Input>) -> LibraryUpdate {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap()
        {
            Input::Library(update) => update,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn sends_cached_then_fresh_and_writes_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(DiskCache::new(dir.path()).unwrap());
        cache
            .write(Section::Playlists.cache_key(), &vec![collection("cached")])
            .unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let source = Arc::new(StubSource {
            calls: AtomicU32::new(0),
            fail: false,
        });
        let requests = spawn_library(source, cache.clone(), tx);
        requests
            .send(LibraryRequest::Reload(Section::Playlists))
            .unwrap();
        assert_eq!(
            next_update(&mut rx).await,
            LibraryUpdate::Section {
                section: Section::Playlists,
                items: vec![collection("cached")]
            }
        );
        assert_eq!(
            next_update(&mut rx).await,
            LibraryUpdate::Section {
                section: Section::Playlists,
                items: vec![collection("fresh")]
            }
        );
        assert_eq!(
            cache.read::<Vec<Collection>>(Section::Playlists.cache_key()),
            Some(vec![collection("fresh")])
        );
    }

    #[tokio::test]
    async fn fresh_cache_is_not_fetched_again() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(DiskCache::new(dir.path()).unwrap());
        cache
            .write(Section::Playlists.cache_key(), &vec![collection("cached")])
            .unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let source = Arc::new(StubSource {
            calls: AtomicU32::new(0),
            fail: false,
        });
        let requests = spawn_library(source.clone(), cache, tx);
        requests
            .send(LibraryRequest::Section(Section::Playlists))
            .unwrap();
        assert_eq!(
            next_update(&mut rx).await,
            LibraryUpdate::Section {
                section: Section::Playlists,
                items: vec![collection("cached")]
            }
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(source.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failure_is_reported_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let source = Arc::new(StubSource {
            calls: AtomicU32::new(0),
            fail: true,
        });
        let requests = spawn_library(source, Arc::new(DiskCache::new(dir.path()).unwrap()), tx);
        requests
            .send(LibraryRequest::Section(Section::Albums))
            .unwrap();
        assert_eq!(
            next_update(&mut rx).await,
            LibraryUpdate::SectionFailed {
                section: Section::Albums,
                reason: FailReason::Offline
            }
        );
    }

    #[tokio::test]
    async fn duplicate_in_flight_requests_fetch_once() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let source = Arc::new(StubSource {
            calls: AtomicU32::new(0),
            fail: false,
        });
        let requests = spawn_library(
            source.clone(),
            Arc::new(DiskCache::new(dir.path()).unwrap()),
            tx,
        );
        requests
            .send(LibraryRequest::Section(Section::Playlists))
            .unwrap();
        requests
            .send(LibraryRequest::Section(Section::Playlists))
            .unwrap();
        next_update(&mut rx).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn albums_are_current_forever_and_playlists_until_their_version_changes() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path()).unwrap();
        let album = "spotify:album:a1";
        assert!(!tracks_current(&cache, &tracks_key(album), album));
        cache
            .write(&tracks_key(album), &Vec::<Track>::new())
            .unwrap();
        assert!(tracks_current(&cache, &tracks_key(album), album));

        let list = "spotify:playlist:p1";
        let listed = |snapshot: &str| {
            vec![Collection {
                uri: list.into(),
                snapshot_id: Some(snapshot.into()),
                ..collection("p1")
            }]
        };
        cache
            .write(Section::Playlists.cache_key(), &listed("s1"))
            .unwrap();
        cache
            .write(&tracks_key(list), &Vec::<Track>::new())
            .unwrap();
        cache.write(&version_key(list), &"s1".to_string()).unwrap();
        assert!(tracks_current(&cache, &tracks_key(list), list));
        cache
            .write(Section::Playlists.cache_key(), &listed("s2"))
            .unwrap();
        assert!(!tracks_current(&cache, &tracks_key(list), list));
    }
}
