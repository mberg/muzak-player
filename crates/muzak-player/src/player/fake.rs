//! Pretend player for `--fake` mode. Shuffle just reverses the queue.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{self, UnboundedSender};

use crate::app::{Input, PlayerCommand, PlayerUpdate};
use crate::library::fake::FakeCatalog;
use crate::model::Track;

pub struct FakePlayer {
    catalog: Arc<FakeCatalog>,
    queue: Vec<Track>,
    index: usize,
    playing: bool,
    position_ms: u32,
}

impl FakePlayer {
    pub fn new(catalog: Arc<FakeCatalog>) -> Self {
        Self {
            catalog,
            queue: Vec::new(),
            index: 0,
            playing: false,
            position_ms: 0,
        }
    }

    pub fn handle(&mut self, command: PlayerCommand) -> Vec<PlayerUpdate> {
        match command {
            PlayerCommand::Load {
                context_uri,
                start_index,
                start_uri,
                shuffle,
            } => {
                self.queue = self.catalog.tracks_for(&context_uri);
                if shuffle {
                    self.queue.reverse();
                }
                if self.queue.is_empty() {
                    self.playing = false;
                    return vec![PlayerUpdate::Stopped];
                }
                let by_uri = start_uri
                    .as_deref()
                    .and_then(|uri| self.queue.iter().position(|t| t.uri == uri));
                self.index = by_uri
                    .unwrap_or(start_index.unwrap_or(0) as usize)
                    .min(self.queue.len() - 1);
                self.start_current()
            }
            PlayerCommand::Play if !self.queue.is_empty() => {
                self.playing = true;
                vec![PlayerUpdate::Playing {
                    position_ms: self.position_ms,
                }]
            }
            PlayerCommand::Play => vec![],
            PlayerCommand::Pause => {
                self.playing = false;
                vec![PlayerUpdate::Paused {
                    position_ms: self.position_ms,
                }]
            }
            PlayerCommand::Next => self.skip(1),
            PlayerCommand::Previous if self.position_ms > 3_000 => {
                self.position_ms = 0;
                vec![PlayerUpdate::Position { position_ms: 0 }]
            }
            PlayerCommand::Previous => self.skip(-1),
            PlayerCommand::Seek { position_ms } => {
                self.position_ms = position_ms;
                vec![PlayerUpdate::Position { position_ms }]
            }
            PlayerCommand::SetVolume { percent } => vec![PlayerUpdate::Volume { percent }],
            PlayerCommand::SetShuffle(shuffle) => vec![PlayerUpdate::Shuffle(shuffle)],
            PlayerCommand::SetRepeat(repeat) => vec![PlayerUpdate::Repeat(repeat)],
        }
    }

    pub fn tick(&mut self, elapsed_ms: u32) -> Vec<PlayerUpdate> {
        if !self.playing {
            return vec![];
        }
        self.position_ms += elapsed_ms;
        if self.position_ms >= self.queue[self.index].duration_ms {
            self.skip(1)
        } else {
            vec![PlayerUpdate::Position {
                position_ms: self.position_ms,
            }]
        }
    }

    fn skip(&mut self, delta: i64) -> Vec<PlayerUpdate> {
        if self.queue.is_empty() {
            return vec![];
        }
        let next = self.index as i64 + delta;
        if next < 0 || next >= self.queue.len() as i64 {
            self.playing = false;
            self.position_ms = 0;
            return vec![PlayerUpdate::Stopped];
        }
        self.index = next as usize;
        self.start_current()
    }

    fn start_current(&mut self) -> Vec<PlayerUpdate> {
        self.playing = true;
        self.position_ms = 0;
        vec![
            PlayerUpdate::Loading,
            PlayerUpdate::TrackChanged(self.queue[self.index].clone()),
            PlayerUpdate::Playing { position_ms: 0 },
        ]
    }
}

pub fn spawn_fake_player(
    catalog: Arc<FakeCatalog>,
    inputs: UnboundedSender<Input>,
) -> UnboundedSender<PlayerCommand> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut player = FakePlayer::new(catalog);
        let send = |updates: Vec<PlayerUpdate>| {
            for update in updates {
                let _ = inputs.send(Input::Player(update));
            }
        };
        send(vec![PlayerUpdate::Connected]);
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                command = rx.recv() => match command {
                    Some(command) => send(player.handle(command)),
                    None => return,
                },
                _ = tick.tick() => send(player.tick(1_000)),
            }
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_with_start_uri_starts_on_that_track() {
        let catalog = Arc::new(FakeCatalog::sample());
        let uri = catalog.albums[0].uri.clone();
        let third = catalog.tracks_for(&uri)[2].clone();
        let mut p = FakePlayer::new(catalog.clone());
        let updates = p.handle(PlayerCommand::Load {
            context_uri: uri,
            start_index: None,
            start_uri: Some(third.uri.clone()),
            shuffle: false,
        });
        assert_eq!(updates[1], PlayerUpdate::TrackChanged(third));
    }

    #[test]
    fn load_plays_and_runs_to_the_end() {
        let catalog = Arc::new(FakeCatalog::sample());
        let uri = catalog.albums[0].uri.clone();
        let mut p = FakePlayer::new(catalog.clone());
        let updates = p.handle(PlayerCommand::Load {
            context_uri: uri.clone(),
            start_index: Some(11),
            start_uri: None,
            shuffle: false,
        });
        assert!(
            matches!(updates[1], PlayerUpdate::TrackChanged(ref t) if t.name.ends_with("Song 12"))
        );
        assert_eq!(updates[2], PlayerUpdate::Playing { position_ms: 0 });
        assert_eq!(
            p.tick(1_000),
            vec![PlayerUpdate::Position { position_ms: 1_000 }]
        );
        assert_eq!(
            p.tick(60_000),
            vec![PlayerUpdate::Stopped],
            "last track ended"
        );
        assert!(p.tick(1_000).is_empty());
    }

    #[test]
    fn unknown_context_stops() {
        let mut p = FakePlayer::new(Arc::new(FakeCatalog::sample()));
        assert_eq!(
            p.handle(PlayerCommand::Load {
                context_uri: "spotify:playlist:nope".into(),
                start_index: None,
                start_uri: None,
                shuffle: false
            }),
            vec![PlayerUpdate::Stopped]
        );
    }
}
