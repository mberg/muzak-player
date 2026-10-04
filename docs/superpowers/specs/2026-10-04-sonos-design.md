# Sonos Remote — Design

Date: 2026-10-04
Status: Agreed in conversation ("evaluate sonor, let's do this")

## Purpose

Let a Muzak player drive the Sonos speakers in the house: pick a room, then browse, search and play exactly as today, with the music coming out of that Sonos room instead of the player's own speaker.

## Principles

- **Local control, no polling Spotify.** Sonos speakers are controlled over the home network with their UPnP/SOAP interface. Spotify's Web API is only used for browsing and search, as today. Playback state comes from speaker events (UPnP GENA), not from asking Spotify.
- **Same core, different player.** The app core already talks to a player through `PlayerCommand` and `PlayerUpdate`. A Sonos player implements the same messages, so screens, play history and the sleep timer keep working.

## Library

`sonor` 2.0 (async, tokio) for discovery, transport, queue, volume and zone groups, plus `rupnp` 2.0's `Service::subscribe` for events. Evaluated against `sonos-sdk`/`sonos-api` 0.9 (active but sync-first and young); `sonor` is small and stable and the Sonos protocol has not changed. It sits behind our own player interface so it can be swapped.

## Playing Spotify on Sonos

Mirrors SoCo's ShareLink plugin, as `sonoscli` does:

| Kind | Enqueued URI | DIDL item id | class |
|---|---|---|---|
| Album | `x-rincon-cpcontainer:1004206c{enc}` | `00040000{enc}` | `object.container.album.musicAlbum` |
| Playlist | `x-rincon-cpcontainer:1006206c{enc}` | `1006206c{enc}` | `object.container.playlistContainer` |
| Track | `x-sonos-spotify:{enc}?sid={svc}&sn=0` | `00032020{enc}` | `object.item.audioItem.musicTrack` |

`{enc}` is the Spotify URI with `:` as `%3a`. The DIDL `desc` is `SA_RINCON{svc}_X_#Svc{svc}-0-Token`, trying service 3079 then 2311. The speaker plays with the Spotify account linked in the Sonos app.

Load: clear the coordinator's queue, add the item, point transport at `x-rincon-queue:{uuid}#0`, seek to the start track, set shuffle, play. Starting at a given song finds it in the queue by its encoded track id. Liked Songs has no container: its cached track list is enqueued song by song. Artists have none either: an artist plays their first cached album.

## State from events

Subscribe to AVTransport and RenderingControl on the group coordinator, renewing before the timeout. `LastChange` gives transport state, play mode, current-track DIDL (title, artist, album, cover, track URI) and duration; RenderingControl gives volume. Position is not evented: it is read with `GetPositionInfo` every couple of seconds while playing — a local call, not Spotify.

## Choosing a room

Settings → Speaker gains Sonos rooms next to the headphone jack and Bluetooth, found with the same "Find speakers" scan. Choosing a room saves it and restarts the player into Sonos mode (the player's own librespot doesn't start). Groups made in the Sonos app are respected: commands go to the room's group coordinator.

## Limits

- Plays with the Sonos-linked Spotify account; picking among several linked family accounts is a follow-up.
- Sonos's local interface is unofficial (Home Assistant and SoCo rely on it).
- Liked Songs enqueues one SOAP call per song (local and fast, capped at 200).
- Untested on hardware until the user is home with Sonos; built against a fake Sonos system and real-format XML.
