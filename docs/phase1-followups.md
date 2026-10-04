# Phase 1 follow-ups

Open items from the per-task and final reviews of phase 1. None blocks a first run on the Pi; fix as they come up or before phase 2.

## Found in Mac testing (2026-10-03)

- Spotify's token service answers 403 "Invalid request" when the librespot session asks for Web API scopes, so the library uses Contingency A (the parent's developer app, `web-auth.json`). Playback still uses the librespot credentials.
- With developer-app tokens, Spotify returns 403 for the tracks of playlists owned by other users (for example followed soundtrack playlists), and for the `/playlists/{id}/tracks` endpoint everywhere. The account's own playlists work through `/items`. The player shows the failed state for the forbidden ones; consider hiding playlists the account does not own, or showing a clearer message.

## Parked from the final review

- After a Wi-Fi drop during a track change, the reconnect resend restarts the collection from track 1 (core resends any `Loading` state with `start_index: None`). Fix: keep a `pending_load` in `Core` set by PlayCollection/PlayTrack, cleared on `Playing`, and resend it with its `start_index`. This also makes an offline track-row tap resume at that track.

## Deferred minor findings

- (Task 1) audio_backend/state_dir not validated (plan doesn't require)
- (Task 1) slint declared in shared + target deps; recheck on first Linux build (Task 13)
- (Task 2) Back on Liked detail is a no-op (empty stack) — intended, untested
- (Task 2) OpenCollection on already-open detail re-requests tracks (harmless, deduped)
- (Task 3) no test for a second notice resetting expiry
- (Task 4) detail Empty/Failed, progress clamp, unknown-collection header, banner None untested
- (Task 4) redundant SectionFailed in grid_status test; Arc annotation in find_collection closure
- (Task 5) bearer token sent to any `next` URL starting with http — restrict to API host
- (Task 5) RecentItem.track non-optional; `items: null` fails page decode
- (Task 5) 429/5xx map to Other (reviewer said this shows "ask a grown-up" — it doesn't; Other doesn't set auth_needed)
- (Task 6) in_flight entry leaks if serve panics (no drop guard)
- (Task 6) service Tracks branch untested
- (Task 6) cache key sanitization lossy (a/b == a:b)
- (Task 7) image cache has no size cap; DefaultHasher fallback name not stable across toolchains; load/spawn_image_loader untested
- (Task 8) full AppState cloned/published every input + tick — check cost on Pi
- (Task 8) fake player quirks (Play after end, Previous on first track stops); thin fake player tests; `pub mod fake;` placement
- (Task 9) scrubber handle jumps back after release until next state publish
- (Task 9) failed covers never retried in-session
- (Task 9) scrolling doesn't reset idle timer — screen can dim mid-scroll (send Touch on pointer events)
- (Task 9) rail label "Liked" vs title "Liked Songs" (brief-mandated); idle fade-in never animates
- (Task 10) probe aborts on network error instead of counting failure; probe creates empty librespot/ dir; OAuth requests only `streaming` scope — if probe 403s, check this first
- (Task 11) session_tokens needs_fresh not cleared when extra-scope request fails (repeats failing request each fetch); pinned token never expiry-checked (one extra 401 per hour)
- (Task 11) auth classify matches librespot login message text (brittle across librespot versions)
- (Task 11) inactive SetVolume/Seek/SetShuffle skipped (UI moves, nothing happens); resume uses last Load's shuffle, not later SetShuffle
- (Task 12) blocking sysfs write on async thread; brightness_for u32 overflow theoretical; Backlight::set untested
- (Task 13) deploy.sh stages binary/config at fixed /tmp paths (not secret); provision.sh leaves remote temp dir if scp fails
