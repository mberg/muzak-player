//! Serves audiobook requests: the library (cached first, then fresh), book pages, sign-in.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc::{self, UnboundedSender};

use super::client::{AUTH_FILE, AbsClient, login, save_auth};
use super::types::{BookDetail, BookProgress, BookSummary, BooksLibrary, Chapter};
use crate::app::{BooksRequest, BooksUpdate, Input};
use crate::library::cache::DiskCache;

/// The signed-in client, shared with the cover loader; None while signed out.
pub type AbsHandle = Arc<RwLock<Option<Arc<AbsClient>>>>;

const LIBRARY_KEY: &str = "books-library";

fn detail_key(id: &str) -> String {
    format!("book-{id}")
}

pub enum Backend {
    Real(AbsHandle),
    /// `--fake`: a pretend server where any sign-in works.
    Fake,
}

pub fn spawn_books(
    backend: Backend,
    state_dir: PathBuf,
    cache: Arc<DiskCache>,
    inputs: UnboundedSender<Input>,
) -> UnboundedSender<BooksRequest> {
    let (tx, mut rx) = mpsc::unbounded_channel::<BooksRequest>();
    let backend = Arc::new(backend);
    tokio::spawn(async move {
        while let Some(request) = rx.recv().await {
            let (backend, state_dir, cache, inputs) = (
                backend.clone(),
                state_dir.clone(),
                cache.clone(),
                inputs.clone(),
            );
            tokio::spawn(async move {
                let send = |update| {
                    let _ = inputs.send(Input::Books(update));
                };
                match &*backend {
                    Backend::Real(handle) => {
                        serve(handle, &state_dir, &cache, request, &send).await
                    }
                    Backend::Fake => serve_fake(request, &send),
                }
            });
        }
    });
    tx
}

async fn serve(
    handle: &AbsHandle,
    state_dir: &std::path::Path,
    cache: &DiskCache,
    request: BooksRequest,
    send: &impl Fn(BooksUpdate),
) {
    let client = handle.read().unwrap().clone();
    match request {
        BooksRequest::Load => {
            if let Some(library) = cache.read::<BooksLibrary>(LIBRARY_KEY) {
                send(BooksUpdate::Library(library));
            }
            let Some(client) = client else { return };
            match client.library().await {
                Ok(library) => {
                    let _ = cache.write(LIBRARY_KEY, &library);
                    send(BooksUpdate::Library(library));
                }
                Err(e) => send(BooksUpdate::LibraryFailed(e.to_string())),
            }
        }
        BooksRequest::Detail(id) => {
            let key = detail_key(&id);
            if let Some((detail, progress)) = cache.read::<(BookDetail, Option<BookProgress>)>(&key)
            {
                send(BooksUpdate::Detail {
                    id: id.clone(),
                    detail,
                    progress,
                });
            }
            let Some(client) = client else { return };
            match client.book(&id).await {
                Ok((detail, progress)) => {
                    let _ = cache.write(&key, &(detail.clone(), progress));
                    send(BooksUpdate::Detail {
                        id,
                        detail,
                        progress,
                    });
                }
                Err(e) => send(BooksUpdate::DetailFailed {
                    id,
                    message: e.to_string(),
                }),
            }
        }
        BooksRequest::SignIn {
            url,
            username,
            password,
        } => match login(&url, &username, &password).await {
            Ok(auth) => {
                let file = state_dir.join(AUTH_FILE);
                if let Err(e) = save_auth(&file, &auth) {
                    tracing::warn!("saving Audiobookshelf sign-in failed: {e}");
                }
                let username = auth.username.clone();
                *handle.write().unwrap() = Some(Arc::new(AbsClient::new(auth, file)));
                send(BooksUpdate::SignedIn { username });
            }
            Err(e) => send(BooksUpdate::SignInFailed(e.to_string())),
        },
        BooksRequest::SignOut => {
            let _ = std::fs::remove_file(state_dir.join(AUTH_FILE));
            let _ = std::fs::remove_file(cache.path_for(LIBRARY_KEY));
            *handle.write().unwrap() = None;
            send(BooksUpdate::SignedOut);
        }
    }
}

fn fake_library() -> BooksLibrary {
    let books: Vec<BookSummary> = [
        (
            "fake-book-1",
            "The Hobbit",
            "J.R.R. Tolkien",
            "Andy Serkis",
            39_000.0,
        ),
        (
            "fake-book-2",
            "Charlotte's Web",
            "E.B. White",
            "Meryl Streep",
            13_200.0,
        ),
        (
            "fake-book-3",
            "Project Hail Mary",
            "Andy Weir",
            "Ray Porter",
            58_000.0,
        ),
        (
            "fake-book-4",
            "Matilda",
            "Roald Dahl",
            "Kate Winslet",
            15_800.0,
        ),
    ]
    .into_iter()
    .map(|(id, title, author, narrator, duration_secs)| BookSummary {
        id: id.into(),
        title: title.into(),
        author: author.into(),
        narrator: narrator.into(),
        duration_secs,
        ..Default::default()
    })
    .collect();
    let mut progress = std::collections::HashMap::new();
    progress.insert(
        "fake-book-3".to_string(),
        BookProgress {
            current_secs: 21_000.0,
            progress: 0.36,
            finished: false,
        },
    );
    BooksLibrary {
        books,
        progress,
        continue_ids: vec!["fake-book-3".into()],
    }
}

fn serve_fake(request: BooksRequest, send: &impl Fn(BooksUpdate)) {
    match request {
        BooksRequest::Load => send(BooksUpdate::Library(fake_library())),
        BooksRequest::Detail(id) => {
            let library = fake_library();
            let Some(summary) = library.books.iter().find(|b| b.id == id).cloned() else {
                return;
            };
            let chapter = summary.duration_secs / 10.0;
            let chapters = (0..10)
                .map(|i| Chapter {
                    title: format!("Chapter {}", i + 1),
                    start_secs: chapter * f64::from(i),
                    end_secs: chapter * f64::from(i + 1),
                })
                .collect();
            send(BooksUpdate::Detail {
                progress: library.progress.get(&id).copied(),
                id,
                detail: BookDetail {
                    summary,
                    description: "A pretend book for trying the Books screens.".into(),
                    chapters,
                },
            });
        }
        BooksRequest::SignIn { username, .. } => send(BooksUpdate::SignedIn { username }),
        BooksRequest::SignOut => send(BooksUpdate::SignedOut),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn next(rx: &mut mpsc::UnboundedReceiver<Input>) -> BooksUpdate {
        match tokio::time::timeout(std::time::Duration::from_secs(20), rx.recv())
            .await
            .expect("an update")
            .unwrap()
        {
            Input::Books(update) => update,
            other => panic!("unexpected {other:?}"),
        }
    }

    /// Against a real server: `ABS_URL=… ABS_USER=… ABS_PASS=… cargo test -- --ignored live_books_service`.
    #[tokio::test]
    #[ignore]
    async fn live_books_service() {
        let (Ok(url), Ok(username), Ok(password)) = (
            std::env::var("ABS_URL"),
            std::env::var("ABS_USER"),
            std::env::var("ABS_PASS"),
        ) else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(DiskCache::new(dir.path().join("cache")).unwrap());
        let handle: AbsHandle = Arc::default();
        let (inputs, mut rx) = mpsc::unbounded_channel();
        let books = spawn_books(
            Backend::Real(handle.clone()),
            dir.path().to_path_buf(),
            cache,
            inputs,
        );
        books
            .send(BooksRequest::SignIn {
                url: url.clone(),
                username: username.clone(),
                password: "wrong".into(),
            })
            .unwrap();
        assert!(matches!(next(&mut rx).await, BooksUpdate::SignInFailed(_)));
        books
            .send(BooksRequest::SignIn {
                url,
                username: username.clone(),
                password,
            })
            .unwrap();
        assert_eq!(next(&mut rx).await, BooksUpdate::SignedIn { username });
        assert!(dir.path().join(AUTH_FILE).is_file());
        books.send(BooksRequest::Load).unwrap();
        let BooksUpdate::Library(library) = next(&mut rx).await else {
            panic!("no library")
        };
        let id = library.books[0].id.clone();
        books.send(BooksRequest::Load).unwrap();
        // The second load shows the cached list first, then the fresh one.
        assert!(matches!(next(&mut rx).await, BooksUpdate::Library(_)));
        assert!(matches!(next(&mut rx).await, BooksUpdate::Library(_)));
        books.send(BooksRequest::Detail(id.clone())).unwrap();
        let BooksUpdate::Detail {
            detail, progress, ..
        } = next(&mut rx).await
        else {
            panic!("no detail")
        };
        println!(
            "{} books; {}: {} chapters, progress {:?}; continue {:?}",
            library.books.len(),
            detail.summary.title,
            detail.chapters.len(),
            progress,
            library.continue_ids
        );
        books.send(BooksRequest::SignOut).unwrap();
        assert_eq!(next(&mut rx).await, BooksUpdate::SignedOut);
        assert!(!dir.path().join(AUTH_FILE).exists());
        assert!(handle.read().unwrap().is_none());
    }
}
