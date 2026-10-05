//! A player that drives a Sonos room over the home network.
//!
//! Commands become UPnP actions on the room's group coordinator; what's playing comes back
//! as UPnP events, so nothing polls Spotify. Position isn't evented, so it's read from the
//! speaker every couple of seconds while playing (a local call).

use std::sync::Arc;
use std::time::Duration;

use futures_util::{Stream, StreamExt};
use rupnp::ssdp::URN;
use sonor::{RepeatMode, Speaker};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use super::protocol::{self, Kind};
use crate::app::{Input, PlayerCommand, PlayerUpdate};
use crate::library::cache::{DiskCache, tracks_key};
use crate::model::{Collection, LIKED_URI, Repeat, Track};
use crate::settings::SonosRoom;

const AV_TRANSPORT: URN = URN::service("schemas-upnp-org", "AVTransport", 1);
const RENDERING_CONTROL: URN = URN::service("schemas-upnp-org", "RenderingControl", 1);
/// Event subscriptions last this long and are renewed well before they lapse.
const SUBSCRIPTION_SECS: u32 = 300;
const RENEW_EVERY: Duration = Duration::from_secs(240);
/// While playing, the position is read this often.
/// How often the speaker is asked for its state and position. Sonos sends events a few
/// seconds late, so the screen follows these answers rather than waiting for events.
const POSITION_EVERY: Duration = Duration::from_secs(1);
/// Liked Songs has no Sonos container, so its songs are queued one by one, up to this many.
const MAX_QUEUED_SONGS: usize = 200;
const DISCOVERY: Duration = Duration::from_secs(5);

pub fn spawn(
    room: SonosRoom,
    cache: Arc<DiskCache>,
    inputs: UnboundedSender<Input>,
) -> UnboundedSender<PlayerCommand> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(run(room, cache, rx, inputs));
    tx
}

/// All Sonos rooms on the network, for the room picker.
pub async fn discover_rooms() -> Vec<SonosRoom> {
    let mut rooms = Vec::new();
    let Ok(speakers) = sonor::discover(DISCOVERY).await else {
        return rooms;
    };
    let mut speakers = std::pin::pin!(speakers);
    while let Some(Ok(speaker)) = speakers.next().await {
        if let (Ok(name), Ok(uuid)) = (speaker.name().await, speaker.uuid().await)
            && rooms.iter().all(|r: &SonosRoom| r.uuid != uuid)
        {
            rooms.push(SonosRoom { uuid, name });
        }
    }
    rooms.sort_by_key(|r| r.name.to_lowercase());
    rooms
}

/// The speaker that leads the room's group: commands must go to it.
async fn find_coordinator(room: &SonosRoom) -> Option<Speaker> {
    let speakers = sonor::discover(DISCOVERY).await.ok()?;
    let mut speakers = std::pin::pin!(speakers);
    let mut found = Vec::new();
    while let Some(Ok(speaker)) = speakers.next().await {
        found.push(speaker);
    }
    let mut uuids = Vec::new();
    for s in &found {
        uuids.push(s.uuid().await.unwrap_or_default());
    }
    // Any speaker can describe every group; ask the first one found.
    let groups = found.first()?.zone_group_state().await.ok()?;
    let coordinator = groups
        .into_iter()
        .find(|(_, members)| members.iter().any(|m| m.uuid() == room.uuid))
        .map(|(coordinator, _)| coordinator)
        .unwrap_or_else(|| room.uuid.clone());
    let index = uuids.iter().position(|u| *u == coordinator)?;
    Some(found.swap_remove(index))
}

fn speaker_base(speaker: &Speaker) -> String {
    let url = speaker.device().url();
    match url.authority() {
        Some(authority) => format!("http://{authority}"),
        None => String::new(),
    }
}

type Events = std::pin::Pin<
    Box<dyn Stream<Item = Result<std::collections::HashMap<String, String>, rupnp::Error>> + Send>,
>;

async fn subscribe(speaker: &Speaker, service: &URN) -> Option<(String, Events)> {
    let device = speaker.device();
    let service = device.find_service(service)?;
    match service.subscribe(device.url(), SUBSCRIPTION_SECS).await {
        Ok((sid, stream)) => Some((sid, Box::pin(stream))),
        Err(e) => {
            tracing::warn!("Sonos event subscription failed: {e}");
            None
        }
    }
}

async fn renew(speaker: &Speaker, service: &URN, sid: &str) {
    let device = speaker.device();
    if let Some(service) = device.find_service(service)
        && let Err(e) = service
            .renew_subscription(device.url(), sid, SUBSCRIPTION_SECS)
            .await
    {
        tracing::warn!("renewing Sonos events failed: {e}");
    }
}

/// What the player has told the core, so events only send real changes.
#[derive(Default)]
struct Seen {
    track_uri: Option<String>,
    playing: bool,
    /// The transport state last told to the core ("PLAYING", "PAUSED_PLAYBACK", ...).
    state: Option<String>,
    position_ms: u32,
    duration_ms: u32,
}

async fn run(
    room: SonosRoom,
    cache: Arc<DiskCache>,
    mut commands: UnboundedReceiver<PlayerCommand>,
    inputs: UnboundedSender<Input>,
) {
    let send = |update| {
        let _ = inputs.send(Input::Player(update));
    };
    loop {
        let Some(speaker) = find_coordinator(&room).await else {
            tracing::warn!("Sonos room {} not found; retrying", room.name);
            send(PlayerUpdate::Disconnected);
            tokio::time::sleep(Duration::from_secs(15)).await;
            continue;
        };
        tracing::info!("playing through Sonos room {}", room.name);
        send(PlayerUpdate::Connected);
        let base = speaker_base(&speaker);
        let uuid = speaker.uuid().await.unwrap_or_else(|_| room.uuid.clone());
        let Some((av_sid, mut av_events)) = subscribe(&speaker, &AV_TRANSPORT).await else {
            send(PlayerUpdate::Disconnected);
            tokio::time::sleep(Duration::from_secs(15)).await;
            continue;
        };
        let rc = subscribe(&speaker, &RENDERING_CONTROL).await;
        let (rc_sid, mut rc_events) = match rc {
            Some((sid, events)) => (Some(sid), Some(events)),
            None => (None, None),
        };
        let mut seen = Seen::default();
        let mut renew_tick = tokio::time::interval(RENEW_EVERY);
        renew_tick.tick().await;
        let mut position_tick = tokio::time::interval(POSITION_EVERY);
        let lost = loop {
            tokio::select! {
                command = commands.recv() => match command {
                    None => return,
                    Some(command) => {
                        let what = format!("{command:?}");
                        if let Err(e) = apply(&speaker, &uuid, &cache, command).await {
                            tracing::warn!("Sonos command failed: {what}: {e}");
                            send(PlayerUpdate::Unavailable);
                        }
                        // Show the result straight away instead of waiting for events.
                        position_tick.reset_immediately();
                    }
                },
                event = av_events.next() => match event {
                    Some(Ok(vars)) => {
                        if let Some(xml) = vars.get("LastChange") {
                            on_transport(&protocol::parse_transport_change(xml), &base, &mut seen, &send);
                        }
                    }
                    _ => break true,
                },
                event = async {
                    match rc_events.as_mut() {
                        Some(events) => events.next().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Some(Ok(vars)) = event
                        && let Some(percent) = vars.get("LastChange").and_then(|x| protocol::parse_volume_change(x))
                    {
                        send(PlayerUpdate::Volume { percent });
                    }
                },
                _ = position_tick.tick() => poll(&speaker, &mut seen, &send).await,
                _ = renew_tick.tick() => {
                    renew(&speaker, &AV_TRANSPORT, &av_sid).await;
                    if let Some(sid) = &rc_sid {
                        renew(&speaker, &RENDERING_CONTROL, sid).await;
                    }
                },
            }
        };
        if lost {
            tracing::warn!("lost Sonos events from {}; reconnecting", room.name);
            send(PlayerUpdate::Disconnected);
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}

/// Asks the speaker for its play state and position, and tells the core what changed.
async fn poll(speaker: &Speaker, seen: &mut Seen, send: &impl Fn(PlayerUpdate)) {
    let device = speaker.device();
    let Some(service) = device.find_service(&AV_TRANSPORT) else {
        return;
    };
    let state = match service
        .action(
            device.url(),
            "GetTransportInfo",
            "<InstanceID>0</InstanceID>",
        )
        .await
    {
        Ok(mut vars) => vars.remove("CurrentTransportState"),
        Err(_) => return,
    };
    if let Ok(Some(info)) = speaker.track().await {
        let position_ms = info.elapsed() * 1000;
        if position_ms != seen.position_ms {
            seen.position_ms = position_ms;
            if seen.playing && state.as_deref() == Some("PLAYING") {
                send(PlayerUpdate::Position { position_ms });
            }
        }
    }
    if state.is_some() && state != seen.state {
        set_state(state.as_deref(), seen, send);
    }
}

/// Tells the core about a transport state it hasn't heard yet.
fn set_state(state: Option<&str>, seen: &mut Seen, send: &impl Fn(PlayerUpdate)) {
    seen.state = state.map(str::to_string);
    match state {
        Some("PLAYING") => {
            seen.playing = true;
            send(PlayerUpdate::Playing {
                position_ms: seen.position_ms,
            });
        }
        Some("PAUSED_PLAYBACK") => {
            seen.playing = false;
            send(PlayerUpdate::Paused {
                position_ms: seen.position_ms,
            });
        }
        Some("STOPPED") => {
            seen.playing = false;
            send(PlayerUpdate::Stopped);
        }
        Some("TRANSITIONING") => send(PlayerUpdate::Loading),
        _ => {}
    }
}

fn on_transport(
    change: &protocol::TransportChange,
    base: &str,
    seen: &mut Seen,
    send: &impl Fn(PlayerUpdate),
) {
    if let Some(duration) = change.duration_ms {
        seen.duration_ms = duration;
    }
    if let Some(didl) = &change.track_didl
        && let Some(track) =
            protocol::parse_track_didl(didl, change.track_uri.as_deref(), seen.duration_ms, base)
        && seen.track_uri.as_deref() != Some(track.uri.as_str())
    {
        seen.track_uri = Some(track.uri.clone());
        seen.position_ms = 0;
        send(PlayerUpdate::TrackChanged(track));
    }
    if let Some(mode) = &change.play_mode {
        let (shuffle, repeat) = protocol::parse_play_mode(mode);
        send(PlayerUpdate::Shuffle(shuffle));
        send(PlayerUpdate::Repeat(repeat));
    }
    if change.state.is_some() && change.state != seen.state {
        set_state(change.state.as_deref(), seen, send);
    }
}

/// Passes a result through, logging which step of a Sonos command failed.
fn step<T>(name: &str, result: Result<T, sonor::Error>) -> Result<T, sonor::Error> {
    if let Err(e) = &result {
        tracing::warn!("Sonos step failed: {name}: {e}");
    }
    result
}

/// Queues a Spotify album, playlist or song, trying each Spotify service type.
async fn enqueue(speaker: &Speaker, uri: &str, title: &str) -> Result<(), sonor::Error> {
    let mut last = None;
    for service in protocol::SPOTIFY_SERVICES {
        let Some((enqueued, didl)) = protocol::enqueue_item(uri, title, service) else {
            break;
        };
        // sonor puts argument values into the SOAP body as they are, so escape them here;
        // unescaped DIDL arrives as stray XML and Sonos answers 500.
        let (uri_arg, didl_arg) = (protocol::xml_escape(&enqueued), protocol::xml_escape(&didl));
        match speaker.queue_end(&uri_arg, &didl_arg).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::warn!("Sonos refused to queue {enqueued} as service {service}: {e}");
                last = Some(e);
            }
        }
    }
    match last {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Loads what the core asked for into the speaker's queue and starts it.
async fn load(
    speaker: &Speaker,
    uuid: &str,
    cache: &DiskCache,
    context_uri: &str,
    start_index: Option<u32>,
    start_uri: Option<&str>,
    shuffle: bool,
) -> Result<(), sonor::Error> {
    step("clear the queue", speaker.clear_queue().await)?;
    match protocol::kind_of(context_uri) {
        Some(Kind::Album | Kind::Playlist | Kind::Track) => {
            enqueue(speaker, context_uri, "").await?
        }
        _ if context_uri == LIKED_URI => {
            // No Sonos container for Liked Songs: queue the cached list.
            let tracks: Vec<Track> = cache.read(&tracks_key(LIKED_URI)).unwrap_or_default();
            for track in tracks.iter().take(MAX_QUEUED_SONGS) {
                enqueue(speaker, &track.uri, &track.name).await?;
            }
        }
        _ => {
            // Artists have no container either: play their first cached album.
            let albums: Vec<Collection> = cache.read(&tracks_key(context_uri)).unwrap_or_default();
            if let Some(album) = albums.first() {
                enqueue(speaker, &album.uri, &album.name).await?;
            }
        }
    }
    step(
        "play from the queue",
        speaker
            .set_transport_uri(&protocol::queue_uri(uuid), "")
            .await,
    )?;
    step("set shuffle", speaker.set_shuffle(shuffle).await)?;
    let track_no = match (start_uri, start_index) {
        (Some(uri), _) => speaker
            .queue()
            .await?
            .iter()
            .position(|t| protocol::spotify_track_in(t.uri()).as_deref() == Some(uri))
            .map(|i| i as u32 + 1),
        (None, Some(index)) => Some(index + 1),
        (None, None) => None,
    };
    if let Some(n) = track_no {
        step("jump to the song", speaker.seek_track(n).await)?;
    }
    step("play", speaker.play().await)
}

async fn apply(
    speaker: &Speaker,
    uuid: &str,
    cache: &DiskCache,
    command: PlayerCommand,
) -> Result<(), sonor::Error> {
    match command {
        PlayerCommand::Load {
            context_uri,
            start_index,
            start_uri,
            shuffle,
        } => {
            load(
                speaker,
                uuid,
                cache,
                &context_uri,
                start_index,
                start_uri.as_deref(),
                shuffle,
            )
            .await
        }
        PlayerCommand::Play => speaker.play().await,
        PlayerCommand::Pause => speaker.pause().await,
        PlayerCommand::Next => speaker.next().await,
        PlayerCommand::Previous => speaker.previous().await,
        PlayerCommand::Seek { position_ms } => speaker.skip_to(position_ms / 1000).await,
        PlayerCommand::SetVolume { percent } => speaker.set_volume(u16::from(percent)).await,
        PlayerCommand::SetShuffle(shuffle) => speaker.set_shuffle(shuffle).await,
        PlayerCommand::SetRepeat(repeat) => {
            speaker
                .set_repeat_mode(match repeat {
                    Repeat::Off => RepeatMode::None,
                    Repeat::Context => RepeatMode::All,
                    Repeat::Track => RepeatMode::One,
                })
                .await
        }
    }
}
