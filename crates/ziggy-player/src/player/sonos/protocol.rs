//! The Sonos side of the wire: Spotify links and DIDL metadata Sonos accepts, and reading
//! its event XML into our types. No network here, so all of it is unit-tested.

use roxmltree::Document;

use crate::model::{Repeat, Track};

/// Spotify's Sonos service types; Sonos varies by region and firmware, so try both.
pub const SPOTIFY_SERVICES: [u32; 2] = [3079, 2311];

/// What a Spotify URI is, for Sonos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Track,
    Album,
    Playlist,
}

pub fn kind_of(uri: &str) -> Option<Kind> {
    match uri.split(':').nth(1)? {
        "track" => Some(Kind::Track),
        "album" => Some(Kind::Album),
        "playlist" => Some(Kind::Playlist),
        _ => None,
    }
}

/// "spotify:track:abc" as Sonos writes it: "spotify%3atrack%3aabc".
pub fn encode(uri: &str) -> String {
    uri.replace(':', "%3a")
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The URI and DIDL metadata to enqueue a Spotify item on Sonos, for one service type.
/// Mirrors the table in SoCo's ShareLink plugin (soco/plugins/sharelink.py): a song is queued
/// by its plain encoded Spotify URI. Sonos then stores it as `x-sonos-spotify:…?sid=12&sn=<n>`
/// with the household's own account number; sending that form ourselves is refused (402).
pub fn enqueue_item(uri: &str, title: &str, service: u32) -> Option<(String, String)> {
    let enc = encode(uri);
    let (enqueued, item_id, class) = match kind_of(uri)? {
        Kind::Album => (
            format!("x-rincon-cpcontainer:1004206c{enc}"),
            format!("00040000{enc}"),
            "object.container.album.musicAlbum",
        ),
        Kind::Playlist => (
            format!("x-rincon-cpcontainer:1006206c{enc}"),
            format!("1006206c{enc}"),
            "object.container.playlistContainer",
        ),
        Kind::Track => (
            enc.clone(),
            format!("00032020{enc}"),
            "object.item.audioItem.musicTrack",
        ),
    };
    let didl = format!(
        concat!(
            r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" "#,
            r#"xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" "#,
            r#"xmlns:r="urn:schemas-rinconnetworks-com:metadata-1-0/" "#,
            r#"xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/">"#,
            r#"<item id="{id}" parentID="-1" restricted="true"><dc:title>{title}</dc:title>"#,
            r#"<upnp:class>{class}</upnp:class>"#,
            r#"<desc id="cdudn" nameSpace="urn:schemas-rinconnetworks-com:metadata-1-0/">"#,
            r#"SA_RINCON{svc}_X_#Svc{svc}-0-Token</desc></item></DIDL-Lite>"#
        ),
        id = xml_escape(&item_id),
        title = xml_escape(title),
        class = class,
        svc = service,
    );
    Some((enqueued, didl))
}

/// The speaker's own queue, which playback is pointed at after enqueueing.
pub fn queue_uri(uuid: &str) -> String {
    format!("x-rincon-queue:{uuid}#0")
}

/// The Spotify track URI inside a Sonos queue or transport URI, if it has one.
pub fn spotify_track_in(sonos_uri: &str) -> Option<String> {
    let decoded = sonos_uri.replace("%3a", ":").replace("%3A", ":");
    let start = decoded.find("spotify:track:")?;
    let id: String = decoded[start + "spotify:track:".len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    (!id.is_empty()).then(|| format!("spotify:track:{id}"))
}

/// "0:03:21" or "00:03:21" in milliseconds.
pub fn parse_hms(s: &str) -> Option<u32> {
    let parts: Vec<u32> = s
        .trim()
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    match parts.as_slice() {
        [h, m, sec] => Some((h * 3600 + m * 60 + sec) * 1000),
        [m, sec] => Some((m * 60 + sec) * 1000),
        _ => None,
    }
}

/// Seconds as Sonos's "H:MM:SS" for seeking.
pub fn format_hms(ms: u32) -> String {
    let s = ms / 1000;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// Sonos play modes as (shuffle, repeat).
pub fn parse_play_mode(mode: &str) -> (bool, Repeat) {
    match mode {
        "REPEAT_ALL" => (false, Repeat::Context),
        "REPEAT_ONE" => (false, Repeat::Track),
        "SHUFFLE_NOREPEAT" => (true, Repeat::Off),
        "SHUFFLE" => (true, Repeat::Context),
        "SHUFFLE_REPEAT_ONE" => (true, Repeat::Track),
        _ => (false, Repeat::Off),
    }
}

pub fn play_mode(shuffle: bool, repeat: Repeat) -> &'static str {
    match (shuffle, repeat) {
        (false, Repeat::Off) => "NORMAL",
        (false, Repeat::Context) => "REPEAT_ALL",
        (false, Repeat::Track) => "REPEAT_ONE",
        (true, Repeat::Off) => "SHUFFLE_NOREPEAT",
        (true, Repeat::Context) => "SHUFFLE",
        (true, Repeat::Track) => "SHUFFLE_REPEAT_ONE",
    }
}

/// What an AVTransport `LastChange` event says.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TransportChange {
    /// "PLAYING", "PAUSED_PLAYBACK", "STOPPED", "TRANSITIONING".
    pub state: Option<String>,
    pub play_mode: Option<String>,
    pub duration_ms: Option<u32>,
    pub track_uri: Option<String>,
    pub track_didl: Option<String>,
}

/// Reads the `val` attributes of a `LastChange` document by element name.
fn last_change_values(xml: &str) -> Vec<(String, String)> {
    let Ok(doc) = Document::parse(xml) else {
        return Vec::new();
    };
    doc.descendants()
        .filter(|n| n.is_element())
        .filter_map(|n| {
            let val = n.attribute("val")?;
            // RenderingControl tags volume with a channel; only Master matters.
            if n.attribute("channel").is_some_and(|c| c != "Master") {
                return None;
            }
            Some((n.tag_name().name().to_string(), val.to_string()))
        })
        .collect()
}

pub fn parse_transport_change(xml: &str) -> TransportChange {
    let mut change = TransportChange::default();
    for (name, val) in last_change_values(xml) {
        match name.as_str() {
            "TransportState" => change.state = Some(val),
            "CurrentPlayMode" => change.play_mode = Some(val),
            "CurrentTrackDuration" => change.duration_ms = parse_hms(&val),
            "CurrentTrackURI" => change.track_uri = Some(val),
            "CurrentTrackMetaData" if !val.is_empty() && val != "NOT_IMPLEMENTED" => {
                change.track_didl = Some(val)
            }
            _ => {}
        }
    }
    change
}

/// Master volume (0–100) from a RenderingControl `LastChange`.
pub fn parse_volume_change(xml: &str) -> Option<u8> {
    last_change_values(xml)
        .into_iter()
        .find(|(name, _)| name == "Volume")
        .and_then(|(_, v)| v.parse::<u8>().ok())
        .map(|v| v.min(100))
}

/// The playing song from Sonos's DIDL metadata. `speaker` (e.g. "http://192.168.1.20:1400")
/// makes relative cover URLs absolute.
pub fn parse_track_didl(
    didl: &str,
    track_uri: Option<&str>,
    duration_ms: u32,
    speaker: &str,
) -> Option<Track> {
    let doc = Document::parse(didl).ok()?;
    let text = |name: &str| {
        doc.descendants()
            .find(|n| n.is_element() && n.tag_name().name() == name)
            .and_then(|n| n.text())
            .map(str::to_string)
    };
    let res = text("res");
    let uri = track_uri
        .and_then(spotify_track_in)
        .or_else(|| res.as_deref().and_then(spotify_track_in))?;
    let image_url = text("albumArtURI").map(|art| {
        if art.starts_with("http") {
            art
        } else {
            format!("{speaker}{art}")
        }
    });
    Some(Track {
        uri,
        name: text("title").unwrap_or_default(),
        artists: text("creator").unwrap_or_default(),
        album: text("album").unwrap_or_default(),
        image_url,
        duration_ms,
        album_uri: None,
        artist_uri: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_playlist_and_track_links_match_what_sonos_expects() {
        let (uri, didl) = enqueue_item("spotify:album:0ETF", "Graceland", 3079).unwrap();
        assert_eq!(uri, "x-rincon-cpcontainer:1004206cspotify%3aalbum%3a0ETF");
        assert!(didl.contains(r#"id="00040000spotify%3aalbum%3a0ETF""#));
        assert!(didl.contains("object.container.album.musicAlbum"));
        assert!(didl.contains("SA_RINCON3079_X_#Svc3079-0-Token"));

        let (uri, _) = enqueue_item("spotify:playlist:p1", "Mix", 3079).unwrap();
        assert_eq!(uri, "x-rincon-cpcontainer:1006206cspotify%3aplaylist%3ap1");

        let (uri, didl) = enqueue_item("spotify:track:t1", "A & B", 2311).unwrap();
        // A Sonos Beam (S2 18.8) refused every x-sonos-spotify: form with 402 and took this.
        assert_eq!(uri, "spotify%3atrack%3at1");
        assert!(didl.contains(r#"id="00032020spotify%3atrack%3at1""#));
        assert!(didl.contains("<dc:title>A &amp; B</dc:title>"));

        assert!(enqueue_item("spotify:artist:a1", "X", 3079).is_none());
        assert_eq!(queue_uri("RINCON_1"), "x-rincon-queue:RINCON_1#0");
    }

    #[test]
    fn spotify_tracks_are_found_in_sonos_uris() {
        assert_eq!(
            spotify_track_in("x-sonos-spotify:spotify%3atrack%3a4W09yvUu?sid=12&flags=8224&sn=2")
                .as_deref(),
            Some("spotify:track:4W09yvUu")
        );
        assert_eq!(spotify_track_in("x-rincon-queue:RINCON_1#0"), None);
    }

    #[test]
    fn times_and_play_modes_round_trip() {
        assert_eq!(parse_hms("0:03:21"), Some(201_000));
        assert_eq!(parse_hms("NOT_IMPLEMENTED"), None);
        assert_eq!(format_hms(3_725_000), "1:02:05");
        for shuffle in [false, true] {
            for repeat in [Repeat::Off, Repeat::Context, Repeat::Track] {
                assert_eq!(
                    parse_play_mode(play_mode(shuffle, repeat)),
                    (shuffle, repeat)
                );
            }
        }
    }

    const DIDL: &str = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" xmlns:r="urn:schemas-rinconnetworks-com:metadata-1-0/" xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/"><item id="-1" parentID="-1" restricted="true"><res protocolInfo="sonos.com-spotify:*:audio/x-spotify:*" duration="0:03:51">x-sonos-spotify:spotify%3atrack%3aABC123?sid=12&amp;flags=8224&amp;sn=2</res><r:streamContent></r:streamContent><upnp:albumArtURI>/getaa?s=1&amp;u=x-sonos-spotify%3aspotify%253atrack%253aABC123</upnp:albumArtURI><dc:title>Doors</dc:title><upnp:class>object.item.audioItem.musicTrack</upnp:class><dc:creator>Noah Kahan</dc:creator><upnp:album>The Great Divide</upnp:album></item></DIDL-Lite>"#;

    #[test]
    fn transport_events_give_state_mode_and_the_song() {
        let escaped = DIDL
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        let xml = format!(
            r#"<Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/"><InstanceID val="0"><TransportState val="PLAYING"/><CurrentPlayMode val="SHUFFLE_NOREPEAT"/><CurrentTrackDuration val="0:03:51"/><CurrentTrackURI val="x-sonos-spotify:spotify%3atrack%3aABC123?sid=12&amp;flags=8224&amp;sn=2"/><CurrentTrackMetaData val="{escaped}"/></InstanceID></Event>"#
        );
        let change = parse_transport_change(&xml);
        assert_eq!(change.state.as_deref(), Some("PLAYING"));
        assert_eq!(change.play_mode.as_deref(), Some("SHUFFLE_NOREPEAT"));
        assert_eq!(change.duration_ms, Some(231_000));
        let track = parse_track_didl(
            change.track_didl.as_deref().unwrap(),
            change.track_uri.as_deref(),
            231_000,
            "http://10.0.0.5:1400",
        )
        .unwrap();
        assert_eq!(track.uri, "spotify:track:ABC123");
        assert_eq!(track.name, "Doors");
        assert_eq!(track.artists, "Noah Kahan");
        assert_eq!(track.album, "The Great Divide");
        assert!(
            track
                .image_url
                .unwrap()
                .starts_with("http://10.0.0.5:1400/getaa?")
        );
    }

    #[test]
    fn volume_events_read_the_master_channel() {
        let xml = r#"<Event xmlns="urn:schemas-upnp-org:metadata-1-0/RCS/"><InstanceID val="0"><Volume channel="Master" val="27"/><Volume channel="LF" val="100"/><Mute channel="Master" val="0"/></InstanceID></Event>"#;
        assert_eq!(parse_volume_change(xml), Some(27));
    }
}
