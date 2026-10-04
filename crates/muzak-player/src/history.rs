//! Play history kept on the device in SQLite (`<state_dir>/history.db`).
//!
//! One row per song started on this player, with everything needed to play it again
//! (no search or Web API call) and how long it actually played. Recent is built from it,
//! and a `synced` column is ready for sending plays to a home service later.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::app::{HistoryCommand, Input};
use crate::model::{PlayRecord, Track};

/// Recent shows a song once it has played this long, so quick skips don't clutter it.
pub const COUNTS_AS_PLAYED_MS: u64 = 30_000;
/// How many plays Recent loads.
pub const RECENT_LIMIT: usize = 200;

pub struct HistoryStore {
    db: Connection,
    /// Row of the song playing now, which `Listened` updates.
    current: Option<i64>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl HistoryStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let db = Connection::open(path)?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS plays (
                id INTEGER PRIMARY KEY,
                played_at INTEGER NOT NULL,
                track_uri TEXT NOT NULL,
                track_json TEXT NOT NULL,
                context_uri TEXT,
                context_name TEXT NOT NULL DEFAULT '',
                context_image TEXT,
                listened_ms INTEGER NOT NULL DEFAULT 0,
                synced INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS plays_played_at ON plays (played_at);",
        )?;
        Ok(Self { db, current: None })
    }

    /// A song started playing.
    pub fn start(&mut self, record: &PlayRecord) -> rusqlite::Result<()> {
        let track_json = serde_json::to_string(&record.track).unwrap_or_default();
        let played_at = if record.played_at > 0 {
            record.played_at
        } else {
            now_secs()
        };
        self.db.execute(
            "INSERT INTO plays (played_at, track_uri, track_json, context_uri, context_name,
                context_image, listened_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                played_at,
                record.track.uri,
                track_json,
                record.context_uri,
                record.context_name,
                record.context_image,
                record.listened_ms as i64,
            ],
        )?;
        self.current = Some(self.db.last_insert_rowid());
        Ok(())
    }

    /// How long the current song has actually played so far.
    pub fn listened(&mut self, listened_ms: u64) -> rusqlite::Result<()> {
        if let Some(id) = self.current {
            self.db.execute(
                "UPDATE plays SET listened_ms = ?1 WHERE id = ?2",
                params![listened_ms as i64, id],
            )?;
        }
        Ok(())
    }

    /// Newest first: songs that played long enough, plus the one playing now.
    pub fn recent(&self, limit: usize) -> rusqlite::Result<Vec<PlayRecord>> {
        let latest: Option<i64> = self
            .db
            .query_row("SELECT MAX(id) FROM plays", [], |r| r.get(0))
            .optional()?
            .flatten();
        let mut stmt = self.db.prepare(
            "SELECT played_at, track_json, context_uri, context_name, context_image, listened_ms
             FROM plays WHERE listened_ms >= ?1 OR id = ?2
             ORDER BY id DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![
                COUNTS_AS_PLAYED_MS as i64,
                latest.unwrap_or(-1),
                limit as i64
            ],
            |r| {
                let track_json: String = r.get(1)?;
                Ok((
                    r.get::<_, i64>(0)?,
                    track_json,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (played_at, track_json, context_uri, context_name, context_image, listened) = row?;
            let Ok(track) = serde_json::from_str::<Track>(&track_json) else {
                continue;
            };
            out.push(PlayRecord {
                track,
                context_uri,
                context_name,
                context_image,
                played_at,
                listened_ms: listened.max(0) as u64,
            });
        }
        Ok(out)
    }
}

/// Runs the store on a blocking thread. Every start sends Recent back, so it stays live.
pub fn spawn_history(
    path: std::path::PathBuf,
    inputs: UnboundedSender<Input>,
) -> UnboundedSender<HistoryCommand> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::task::spawn_blocking(move || run(&path, rx, inputs));
    tx
}

fn run(
    path: &Path,
    mut commands: UnboundedReceiver<HistoryCommand>,
    inputs: UnboundedSender<Input>,
) {
    let mut store = match HistoryStore::open(path) {
        Ok(store) => store,
        Err(e) => {
            tracing::warn!("play history unavailable ({}): {e}", path.display());
            let _ = inputs.send(Input::HistoryRecent(Vec::new()));
            return;
        }
    };
    let send_recent = |store: &HistoryStore| match store.recent(RECENT_LIMIT) {
        Ok(plays) => {
            let _ = inputs.send(Input::HistoryRecent(plays));
        }
        Err(e) => tracing::warn!("reading play history failed: {e}"),
    };
    while let Some(command) = commands.blocking_recv() {
        let result = match command {
            HistoryCommand::Start(record) => {
                let result = store.start(&record);
                send_recent(&store);
                result
            }
            HistoryCommand::Listened(ms) => store.listened(ms),
            HistoryCommand::LoadRecent => {
                send_recent(&store);
                Ok(())
            }
        };
        if let Err(e) = result {
            tracing::warn!("writing play history failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(n: u32) -> PlayRecord {
        PlayRecord {
            track: Track {
                uri: format!("spotify:track:t{n}"),
                name: format!("Song {n}"),
                artists: "Band".into(),
                album: "Album".into(),
                image_url: None,
                duration_ms: 180_000,
                album_uri: Some("spotify:album:a1".into()),
                artist_uri: None,
            },
            context_uri: Some("spotify:album:a1".into()),
            context_name: "Album".into(),
            context_image: None,
            played_at: 1_000 + i64::from(n),
            listened_ms: 0,
        }
    }

    #[test]
    fn recent_keeps_songs_that_played_long_enough_and_the_current_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::open(&dir.path().join("history.db")).unwrap();
        store.start(&record(1)).unwrap();
        store.listened(45_000).unwrap();
        store.start(&record(2)).unwrap(); // skipped after 5s
        store.listened(5_000).unwrap();
        store.start(&record(3)).unwrap(); // playing now
        let names: Vec<String> = store
            .recent(10)
            .unwrap()
            .into_iter()
            .map(|p| p.track.name)
            .collect();
        assert_eq!(names, ["Song 3", "Song 1"]);
    }

    #[test]
    fn plays_survive_reopening_with_full_track_details() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        {
            let mut store = HistoryStore::open(&path).unwrap();
            store.start(&record(1)).unwrap();
            store.listened(60_000).unwrap();
        }
        let store = HistoryStore::open(&path).unwrap();
        let plays = store.recent(10).unwrap();
        assert_eq!(plays.len(), 1);
        assert_eq!(
            plays[0].track.album_uri.as_deref(),
            Some("spotify:album:a1")
        );
        assert_eq!(plays[0].listened_ms, 60_000);
        assert_eq!(plays[0].played_at, 1_001);
    }
}
