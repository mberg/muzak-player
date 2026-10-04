//! Voice requests: lower the music while listening, then run what was asked like a tap.

use super::*;

/// How far the music is lowered while listening, as a share of its volume.
const DUCK_PERCENT: u8 = 25;
/// A step for "louder" and "quieter".
const VOLUME_STEP: u8 = 15;
/// After this long without a reply, give up and bring the music back.
const VOICE_TIMEOUT_MS: u64 = 20_000;
/// At most this many library items go to Gemini.
const MAX_ITEMS: usize = 500;

impl Core {
    pub(super) fn on_voice(&mut self, update: VoiceUpdate, now_ms: u64, fx: &mut Vec<Effect>) {
        match update {
            VoiceUpdate::Woke => {
                self.wake(now_ms, fx);
                self.set_voice_phase(VoicePhase::Listening, now_ms);
                let pb = &self.state.playback;
                if self.state.voice.ducked_from.is_none() && pb.status == PlayStatus::Playing {
                    let volume = pb.volume;
                    self.state.voice.ducked_from = Some(volume);
                    let quiet = (u16::from(volume) * u16::from(DUCK_PERCENT) / 100) as u8;
                    fx.push(Effect::Player(PlayerCommand::SetVolume { percent: quiet }));
                }
                fx.push(Effect::VoiceContext(self.voice_context()));
            }
            VoiceUpdate::Thinking => self.set_voice_phase(VoicePhase::Thinking, now_ms),
            VoiceUpdate::Command(command) => {
                self.set_voice_phase(VoicePhase::Idle, now_ms);
                self.run_voice_command(command, now_ms, fx);
            }
            VoiceUpdate::NotUnderstood => {
                self.end_voice(now_ms, fx);
                self.notify(Notice::Voice("Didn't catch that".into()), now_ms);
            }
            VoiceUpdate::Failed(message) => {
                self.end_voice(now_ms, fx);
                self.notify(Notice::Voice(message), now_ms);
            }
        }
    }

    fn set_voice_phase(&mut self, phase: VoicePhase, now_ms: u64) {
        self.state.voice.phase = phase;
        self.state.voice.since_ms = now_ms;
    }

    /// Back to idle with the music at its old volume.
    fn end_voice(&mut self, now_ms: u64, fx: &mut Vec<Effect>) {
        self.set_voice_phase(VoicePhase::Idle, now_ms);
        if let Some(volume) = self.state.voice.ducked_from.take() {
            self.on_ui(UiAction::SetVolume { percent: volume }, now_ms, fx);
        }
    }

    /// Called every second: a request that never got a reply mustn't leave the music quiet.
    pub(super) fn voice_tick(&mut self, now_ms: u64, fx: &mut Vec<Effect>) {
        let voice = &self.state.voice;
        if voice.phase != VoicePhase::Idle
            && now_ms.saturating_sub(voice.since_ms) >= VOICE_TIMEOUT_MS
        {
            self.end_voice(now_ms, fx);
        }
    }

    fn run_voice_command(&mut self, command: VoiceCommand, now_ms: u64, fx: &mut Vec<Effect>) {
        // Volume changes start from the volume before the music was lowered.
        let volume = self
            .state
            .voice
            .ducked_from
            .unwrap_or(self.state.playback.volume);
        match command {
            VoiceCommand::Louder => {
                self.state.voice.ducked_from = None;
                let percent = volume.saturating_add(VOLUME_STEP).min(100);
                self.on_ui(UiAction::SetVolume { percent }, now_ms, fx);
                return;
            }
            VoiceCommand::Quieter => {
                self.state.voice.ducked_from = None;
                let percent = volume.saturating_sub(VOLUME_STEP);
                self.on_ui(UiAction::SetVolume { percent }, now_ms, fx);
                return;
            }
            VoiceCommand::SetVolume(percent) => {
                self.state.voice.ducked_from = None;
                self.on_ui(UiAction::SetVolume { percent }, now_ms, fx);
                return;
            }
            _ => self.end_voice(now_ms, fx),
        }
        let status = self.state.playback.status;
        match command {
            VoiceCommand::Play(uri) => {
                let Some(item) = self
                    .voice_context()
                    .items
                    .into_iter()
                    .find(|i| i.uri == uri)
                else {
                    // Never play something Gemini made up.
                    self.notify(Notice::Voice("Didn't catch that".into()), now_ms);
                    return;
                };
                if item.kind == "artist" {
                    self.on_ui(UiAction::PlayArtist(uri), now_ms, fx);
                } else {
                    let shuffle = false;
                    self.on_ui(UiAction::PlayCollection { uri, shuffle }, now_ms, fx);
                }
                self.notify(Notice::Voice(format!("Playing {}", item.name)), now_ms);
            }
            VoiceCommand::Search { query, kind } => {
                let query = query.trim().to_string();
                if query.is_empty() {
                    self.notify(Notice::Voice("Didn't catch that".into()), now_ms);
                    return;
                }
                let search = &mut self.state.search;
                search.query = query.clone();
                search.sent = query.clone();
                search.edited_ms = now_ms;
                search.results = Slot {
                    loading: true,
                    ..Slot::default()
                };
                self.state.voice.pending_search = Some((query.clone(), kind));
                fx.push(Effect::Library(LibraryRequest::Search(query.clone())));
                self.notify(Notice::Voice(format!("Looking for {query}")), now_ms);
            }
            VoiceCommand::Pause => {
                if matches!(status, PlayStatus::Playing | PlayStatus::Loading) {
                    self.on_ui(UiAction::TogglePlay, now_ms, fx);
                }
            }
            VoiceCommand::Resume => {
                if matches!(status, PlayStatus::Paused | PlayStatus::Stopped) {
                    self.on_ui(UiAction::TogglePlay, now_ms, fx);
                }
            }
            VoiceCommand::Next => self.on_ui(UiAction::Next, now_ms, fx),
            VoiceCommand::Previous => self.on_ui(UiAction::Previous, now_ms, fx),
            VoiceCommand::SleepTimer(minutes) => {
                let minutes = (minutes > 0).then_some(minutes);
                self.on_ui(UiAction::SetSleepTimer(minutes), now_ms, fx);
            }
            VoiceCommand::Louder | VoiceCommand::Quieter | VoiceCommand::SetVolume(_) => {}
        }
    }

    /// Search results arrived: play the first of the kind a voice search asked for.
    pub(super) fn voice_search_results(&mut self, query: &str, now_ms: u64, fx: &mut Vec<Effect>) {
        let Some((wanted, kind)) = self.state.voice.pending_search.clone() else {
            return;
        };
        if wanted != query {
            return;
        }
        self.state.voice.pending_search = None;
        let Some(results) = self.state.search.results.data.clone() else {
            return;
        };
        let action = match kind {
            SearchKind::Song => results
                .tracks
                .first()
                .map(|t| UiAction::PlaySong(t.uri.clone())),
            SearchKind::Artist => results
                .artists
                .first()
                .map(|a| UiAction::PlayArtist(a.uri.clone())),
            SearchKind::Album => results.albums.first().map(|a| UiAction::PlayCollection {
                uri: a.uri.clone(),
                shuffle: false,
            }),
            SearchKind::Playlist => results.playlists.first().map(|p| UiAction::PlayCollection {
                uri: p.uri.clone(),
                shuffle: false,
            }),
        };
        let name = match kind {
            SearchKind::Song => results
                .tracks
                .first()
                .map(|t| format!("{} by {}", t.name, t.artists)),
            SearchKind::Artist => results.artists.first().map(|a| a.name.clone()),
            SearchKind::Album => results
                .albums
                .first()
                .map(|a| format!("{} by {}", a.name, a.subtitle)),
            SearchKind::Playlist => results.playlists.first().map(|p| p.name.clone()),
        };
        match (action, name) {
            (Some(action), Some(name)) => {
                self.on_ui(action, now_ms, fx);
                self.notify(Notice::Voice(format!("Playing {name}")), now_ms);
            }
            _ => self.notify(Notice::Voice(format!("Couldn't find {query}")), now_ms),
        }
    }

    pub(super) fn voice_search_failed(&mut self, query: &str) {
        if self
            .state
            .voice
            .pending_search
            .as_ref()
            .is_some_and(|(q, _)| q == query)
        {
            self.state.voice.pending_search = None;
        }
    }

    /// The library and what's playing, for Gemini.
    pub fn voice_context(&self) -> VoiceContext {
        let mut items: Vec<VoiceItem> = Vec::new();
        let sections = [
            Section::Playlists,
            Section::Albums,
            Section::Artists,
            Section::Liked,
            Section::Recent,
        ];
        let collections = sections
            .iter()
            .filter_map(|s| self.state.sections.get(s))
            .filter_map(|slot| slot.data.as_ref())
            .flat_map(|list| list.iter())
            .chain(self.state.artists_seen.values());
        for c in collections {
            if items.len() >= MAX_ITEMS {
                break;
            }
            if items.iter().any(|i| i.uri == c.uri) {
                continue;
            }
            items.push(VoiceItem {
                uri: c.uri.clone(),
                kind: match c.kind {
                    CollectionKind::Playlist => "playlist",
                    CollectionKind::Album => "album",
                    CollectionKind::Liked => "liked",
                    CollectionKind::Artist => "artist",
                },
                name: c.name.clone(),
                by: c.subtitle.clone(),
            });
        }
        let pb = &self.state.playback;
        VoiceContext {
            now_playing: pb
                .track
                .as_ref()
                .map(|t| format!("{} by {}", t.name, t.artists)),
            playing: pb.status == PlayStatus::Playing,
            volume: self.state.voice.ducked_from.unwrap_or(pb.volume),
            items,
        }
    }
}
