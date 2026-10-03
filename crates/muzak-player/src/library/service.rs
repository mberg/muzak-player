//! Serves library requests: cached data first, then a fresh fetch.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::{self, UnboundedSender};

use super::LibrarySource;
use super::cache::{DiskCache, tracks_key};
use crate::app::{Input, LibraryRequest, LibraryUpdate};
use crate::model::{Collection, Track};

pub fn spawn_library<S: LibrarySource>(
    source: Arc<S>,
    cache: Arc<DiskCache>,
    inputs: UnboundedSender<Input>,
) -> UnboundedSender<LibraryRequest> {
    let (tx, mut rx) = mpsc::unbounded_channel::<LibraryRequest>();
    let in_flight = Arc::new(Mutex::new(HashSet::<LibraryRequest>::new()));
    tokio::spawn(async move {
        while let Some(request) = rx.recv().await {
            if !in_flight.lock().unwrap().insert(request.clone()) {
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
    match request {
        LibraryRequest::Section(section) => {
            let key = section.cache_key();
            if let Some(items) = cache.read::<Vec<Collection>>(key) {
                send(LibraryUpdate::Section { section, items });
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
        LibraryRequest::Tracks { collection_uri } => {
            let key = tracks_key(&collection_uri);
            if let Some(tracks) = cache.read::<Vec<Track>>(&key) {
                send(LibraryUpdate::Tracks {
                    collection_uri: collection_uri.clone(),
                    tracks,
                });
            }
            match source.tracks(&collection_uri).await {
                Ok(tracks) => {
                    if let Err(e) = cache.write(&key, &tracks) {
                        tracing::warn!("cache write failed for {key}: {e}");
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
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use super::*;
    use crate::app::FailReason;
    use crate::library::FetchError;
    use crate::model::{Collection, CollectionKind, Section, Track};

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
            .send(LibraryRequest::Section(Section::Playlists))
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
}
