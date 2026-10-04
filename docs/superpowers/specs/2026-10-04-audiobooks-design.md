# Audiobooks (Audiobookshelf) — Scope and Design

Date: 2026-10-04
Status: Scoping draft for review

## Goal

An optional Books section that plays audiobooks from an [Audiobookshelf](https://audiobookshelf.org) (ABS) server on the home network: browse and search the library, play a book, and always resume where the listener left off, on this player or any other ABS app. Off by default; Spotify users who don't run ABS never see it.

## What Audiobookshelf gives us

Checked against the server source (v2.37.1); the published API docs say they're out of date.

| Need | Endpoint |
|---|---|
| Sign in | `POST /login` `{username, password}` with header `x-return-tokens: true` → `accessToken` + `refreshToken` |
| Stay signed in | `POST /auth/refresh` with header `x-refresh-token`; refresh tokens rotate on every use (access ~12 h, refresh ~7 days by default, server-configurable) |
| Libraries | `GET /api/libraries` (each has `mediaType`: `book` or `podcast`) |
| Books, paged and sorted | `GET /api/libraries/:id/items?limit&page&sort&desc&minified=1` |
| Search | `GET /api/libraries/:id/search?q=` (books, authors, series, narrators) |
| Continue listening, recently added | `GET /api/libraries/:id/personalized` (shelves such as `continue-listening`, `newest-items`) |
| One book (chapters, files, duration) | `GET /api/items/:id?expanded=1` |
| Cover | `GET /api/items/:id/cover` |
| Start listening | `POST /api/items/:id/play` with `deviceInfo`, `supportedMimeTypes`, `forceDirectPlay` → session with `audioTracks[]` (`contentUrl` `/api/items/:id/file/:ino`, `startOffset`, `duration`), `chapters[]`, `currentTime` (the resume point) |
| Save position | `POST /api/session/:id/sync` `{currentTime, timeListened, duration}`; `POST /api/session/:id/close` at the end |
| Offline listening caught up later | `POST /api/session/local` |
| Progress, finished | `GET/PATCH /api/me/progress/:itemId` (`currentTime`, `isFinished`) |
| Bookmarks | `POST/PATCH/DELETE /api/me/item/:id/bookmark` |

The server reconciles progress itself: an older sync never overwrites newer progress from another device.

**Auth note.** API keys exist (admin-made, v2.26+) but are aimed at scripts and changed header between versions (`Bearer` before 2.35, `X-Api-Key` after). Username and password with refresh tokens is what the official and third-party apps use, so that's the main path; an API key in the config file is a fallback for a shared household account.

## What other players do

The official apps and third-party players (ShelfPlayer and Plappa on iOS, Lissen on Android) converge on: continue listening at the top; chapters as the main navigation; skip back/forward (often 15 or 30 s); playback speed; a sleep timer with "end of chapter"; bookmarks; offline downloads; series and authors. That shapes the phases below.

## Rust libraries

- **No Audiobookshelf client crate exists**; a thin `reqwest` client like our Spotify one is a day or two (the endpoints above).
- **Playback** needs our own audio path (librespot only plays Spotify): `stream-download` (seekable HTTP streaming with Range requests and a bounded cache) → `symphonia` (MP3, AAC-LC in M4B/M4A, FLAC, Ogg) → `rodio`/`cpal` (Core Audio on a Mac, ALSA on the Pi, so Bluetooth and a DAC work the same as Spotify). All three are actively maintained.
- **HE-AAC** books aren't decoded by Symphonia; `symphonia-adapter-fdk-aac` covers them, or the server can transcode (`forceTranscode`).
- **Speed without chipmunk voices** needs time-stretching, e.g. `signalsmith-stretch` (Rust bindings to a well-regarded C++ library). Needs a CPU check on the Pi 3.

## The experience

- **Settings → Audiobooks** (hidden until turned on): on/off, server address, sign in with username and password on the on-screen keyboard, signed-in user, Sign out. The same can come from the device config file (`[audiobookshelf] url = …`) plus a `muzak-setup abs-login` command on the Mac, matching how Spotify is set up.
- **Rail:** a single **Books** item appears only when Audiobookshelf is on.
- **Books screen:** a search field at the top (on-screen keyboard), a **Continue listening** row, then the whole library as tiles or a list (the existing toggle), sorted by title, author or recently added.
- **Book page:** cover, title, author, narrator, series, length, progress ("2 h 10 min left"), **Resume** or **Play**, the chapter list (tap to jump), Mark finished.
- **Book player** (Now Playing for books): cover, book and chapter title, a chapter scrubber plus time left in the book, back 30 s / forward 30 s, previous/next chapter, play/pause, speed (0.8–2×), volume, and the moon sleep timer with an extra "end of chapter" option.
- **Mini player** shows whichever is playing: a song or a book.
- **One thing at a time:** starting a book pauses Spotify and vice versa.

## Resume and progress

- Starting a book opens an ABS playback session; its `currentTime` is where playback starts, so a book half-listened on a phone resumes exactly there.
- While playing, sync every 15 s, and immediately on pause, skip, chapter change, sleep-timer stop and app exit.
- If the server is unreachable, keep the position locally (in the existing SQLite history file) and send it with `/api/session/local` when it's back.
- Book listening goes into this player's play history too, so Recent and future stats include books.

## Architecture

- `audiobooks` module: ABS client (sign-in, refresh with rotation saved to `abs-auth.json`, libraries, items, search, sessions, progress), cached on disk like the Spotify library.
- `player/local`: the streaming decoder and audio output, as a second player alongside librespot, driven by the same kind of commands and updates.
- Core: a `books` slice of state (library, search, current book, session, chapter, speed) and a `source: Spotify | Book` for the mini player, sleep timer and pause-the-other rule. Screens: Books, Book, Book player.
- Settings: `audiobookshelf: Option<{url, enabled}>` in `settings.json`; tokens in `abs-auth.json` (0600).

## Phases

1. **Connect and browse.** Settings and config, sign-in and refresh, Books rail item, library list, search, book page with progress. Nothing plays yet. *(about the size of the search PR)*
2. **Listen and resume.** Streaming decoder and output, session start/sync/close, resume, back/forward 30 s, scrubber, book player screen, mini player, Spotify/book hand-off, offline position catch-up. *(the biggest and riskiest step)*
3. **Chapters and comfort.** Chapter list and navigation, speed with time-stretch, sleep timer "end of chapter", Continue listening, Mark finished.
4. **Later, if wanted.** Offline downloads, bookmarks, series and authors pages, podcasts (ABS serves them through the same API), playing books on a Sonos room.

## Risks

- **Sharing the audio device.** librespot and the book player both use the Pi's ALSA output; Spotify must release it while a book plays. Needs an early spike on the Pi, including Bluetooth.
- **Pi 3 headroom.** Decoding AAC is light; time-stretching for speed is the CPU risk, so speed is in phase 3 after measuring.
- **Big files on a small SD card.** Stream with a bounded cache rather than downloading whole books (often 300 MB–1 GB).
- **Token expiry.** Refresh tokens last ~7 days by default; a player left off longer than that needs signing in again. The settings screen will say so; an API key avoids it.
- **API drift.** The docs are stale and auth changed twice in a year; the client is written against the server source and tested with recorded responses.

## Decisions (2026-10-04)

1. **Sign-in: both.** On the device with the on-screen keyboard, and from the Mac with `muzak-setup abs-login`.
2. **One Audiobookshelf user per player.** The home server gets a user per person, so each player keeps its own progress.
3. **Skip: 30 seconds** back and forward.
4. **No podcasts** for now; only libraries whose `mediaType` is `book`.

Search on the Books screen filters the already-loaded library on the device (title, author, narrator, series), so typing is instant and costs no server requests; a home library is a few thousand books at most.
