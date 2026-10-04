# Search and Playlist Editing — Design

Date: 2026-10-03
Status: Approved in conversation, ready for an implementation plan
Replaces: phase 3 ("Playlist creation and editing") in [the main design](2026-10-03-muzak-player-design.md), and pulls typed search forward from phase 4.

## Purpose

Let the user find any song on Spotify and build their own playlists from the touchscreen, with no other device.

Success means:

- The user can type a search on the screen and play a song, album or playlist from the results.
- From any track list or Now Playing, the user can add the song to one of their playlists, or to a new one they name on the screen.
- On a playlist they own, the user can remove songs, reorder them, rename the playlist and delete it.
- Every edit shows on screen at once and survives a restart. If Spotify refuses an edit, the screen goes back to how it was and says so.

The audience is anyone with a Spotify Premium account, adults included. Screens, docs and command output say "you", not "kid" or "parent".

## Scope

In:

- An on-screen keyboard.
- A Search rail item with Songs, Albums and Playlists results.
- Add a song to a playlist, from track rows and Now Playing.
- Create a playlist, named with the keyboard.
- On playlists the user owns: remove a song, reorder by drag, rename, delete.
- New Web API scopes, and a write check in `muzak-setup probe`.

Out:

- Artist results and an artist screen.
- Liking or unliking songs, and saving albums.
- Editing playlists the user does not own, including collaborative ones.
- Playlist descriptions, cover images, and public or private switches. New playlists are private.
- Queuing edits made offline.
- Voice (still phase 2).

## Delivery

Three pull requests, each usable on its own:

1. **Keyboard and Search.** Also: write support in the HTTP layer, and removing kid and parent wording from the README and `muzak-setup`.
2. **Add to playlist and create.** Also: new scopes, owner tracking, and the probe write check.
3. **Edit own playlists.** Remove, reorder, rename, delete.

## Architecture

The existing shape stays: the UI sends taps, the pure app core turns them into effects, and the runtime carries the effects out. New work follows the same paths.

```
ui ── UiAction ──► Core ── Effect::Library(LibraryRequest) ──► library service ── reads ──► Web API
                     ▲                                                    └──── writes ──► Web API
                     └──────────── Input::Library(LibraryUpdate) ◄────────────┘
```

### App core

New state in `AppState`:

- `screen` gains `Screen::Search`. `Section` gains `Search`, the fifth rail item.
- `search: SearchState` holds the query text, the results (`Slot<SearchResults>`), and the query the results belong to.
- `keyboard: Option<KeyboardTarget>`. When set, the keyboard is open. The target says what the text is for: `Search`, `NewPlaylist { track_uri }`, or `Rename { playlist_uri }`. Its text buffer lives here.
- `picker: Option<String>` holds the track URI while the add-to-playlist picker is open.
- `editing: Option<String>` holds the URI of the playlist in edit mode.
- `confirm_delete: Option<String>` holds the playlist URI while the delete confirmation is up.
- `me: Option<String>` holds the user's Spotify ID, used to decide which playlists are editable.

New `UiAction`s:

- Keyboard: `KeyboardOpen(KeyboardTarget)`, `KeyPressed(char)`, `Backspace`, `KeyboardDone`, `KeyboardCancel`, `TextChanged(String)` (from the Mac's physical keyboard).
- Search: `OpenSearchResult { uri }`, `PlaySearchTrack { index }`.
- Playlists: `OpenPicker { track_uri }`, `PickPlaylist { playlist_uri }`, `ClosePicker`, `EditPlaylist(uri)`, `FinishEditing`, `RemoveTrack { index }`, `MoveTrack { from, to }`, `RenamePlaylist`, `AskDelete`, `ConfirmDelete`, `CancelDelete`.

Search timing: `KeyPressed` and `Backspace` update the text and record the time. `Tick` is once a second, which is too slow, so the runtime adds a 100ms `SearchTick` while the keyboard is open for search. The core sends `LibraryRequest::Search(query)` when the text has been unchanged for 400ms and differs from the last query sent. Empty text clears the results without a request. Results for a query that is no longer current are dropped.

Edits are optimistic. For each edit the core:

1. changes `tracks` or `sections` in state at once;
2. keeps the previous value under an edit ID;
3. sends `LibraryRequest::Edit { id, edit }`.

`LibraryUpdate::EditDone { id }` drops the saved value. `LibraryUpdate::EditFailed { id, reason }` puts it back and shows a notice: "No internet right now" for `Offline`, otherwise "Couldn't save that". While the app is offline, edit actions show "No internet right now" and change nothing.

Adding a song that the playlist's loaded track list already contains does nothing except show "Already in <name>". If the track list is not loaded, the add goes ahead.

The picker lists playlists from the Playlists section whose owner is `me`, with "New playlist" first. `PickPlaylist` adds the song, closes the picker, and shows "Added to <name>".

Creating a playlist: `KeyboardDone` on a `NewPlaylist` target with non-empty text sends one edit, `CreateAndAdd { name, track_uri }`. The core inserts a placeholder playlist at the top of the Playlists section with a temporary URI. On success the update carries the real URI, which replaces the placeholder. Names are trimmed and limited to 100 characters.

Delete removes the playlist from the Playlists and Recent sections, leaves edit mode, and goes back to the Playlists grid. If the deleted playlist is playing, playback continues.

### Model

- `Collection` gains `owner_id: Option<String>` and `snapshot_id: Option<String>`. Both are `#[serde(default)]`, so existing caches still load.
- `CollectionKind::Playlist` collections are editable when `owner_id == me`.
- New `SearchResults { tracks: Vec<Track>, albums: Vec<Collection>, playlists: Vec<Collection> }`, at most 20 of each.
- New `PlaylistEdit` enum: `Add { playlist_uri, track_uri }`, `CreateAndAdd { name, track_uri }`, `Remove { playlist_uri, track_uri, position, snapshot_id }`, `Move { playlist_uri, from, to, snapshot_id }`, `Rename { playlist_uri, name }`, `Delete { playlist_uri }`.

### Library

- `Http` gains `send_json(method, url, token, body) -> Result<Value, HttpError>` for POST, PUT and DELETE. An empty response body decodes as `Value::Null`.
- `WebApi` gains `me()`, `search(query)`, and `apply(edit)`. The 401 retry in `get` moves into a shared helper used by both reads and writes.
- `LibrarySource` gains `search` and `apply`. `FakeSource` implements them on the fake catalog, so everything works with `--fake`.
- The library service handles `LibraryRequest::Search` without the cache, and `LibraryRequest::Edit` by calling `apply`, then rewriting the affected cache entries: the playlist's track list, and the Playlists section for create, rename and delete. Edits are never deduplicated as in-flight requests, because two identical adds are two adds.
- At startup the runtime asks for `me` once through `LibraryRequest::Me`. Until it arrives, the picker shows only "New playlist" and Edit buttons stay hidden.

Endpoints. Spotify renamed some playlist endpoints in 2026, so each write tries the `items` form first and falls back on 404, as reads already do:

| Edit | Request |
|---|---|
| Search | `GET /search?type=track,album,playlist&limit=20&q=…` |
| Me | `GET /me` |
| Add | `POST /playlists/{id}/items` with `uris` |
| Create | `POST /me/playlists`, falling back to `POST /users/{me}/playlists`, then Add |
| Remove | `DELETE /playlists/{id}/items` with `items: [{uri, positions:[n]}]` and `snapshot_id` |
| Move | `PUT /playlists/{id}/items` with `range_start`, `insert_before`, `snapshot_id` |
| Rename | `PUT /playlists/{id}` with `name` |
| Delete | `DELETE /playlists/{id}/followers` |

Responses that return a new `snapshot_id` update the stored collection, so the next remove or move uses it.

### Scopes and setup

- `SCOPES` in both crates gains `playlist-modify-private` and `playlist-modify-public`. Existing `web-auth.json` files lack them, so writes return 403 until the user re-runs `muzak-setup auth-web`. The README says so. A 403 on a write maps to "Couldn't save that".
- `muzak-setup probe --write` creates a private playlist named "Muzak probe", adds one track, removes it, renames it, and deletes it, printing each status. Without `--write` the probe stays read-only.

## UI

All sizes fit 800×480 with 48px touch targets or larger.

- **Keyboard.** Fills the bottom 240px of the screen, over the content and mini player. Four rows: digits, then three QWERTY letter rows, then a bottom row of space, backspace and Done. A text field sits above it with a Cancel button. Keys show a pressed state. On the Mac a hidden `TextInput` takes the physical keyboard and sends `TextChanged`.
- **Search screen.** Opening it opens the keyboard. Results show in the top 240px while typing, and fill the screen after Done. Songs are rows with play-on-tap and a plus button; albums and playlists are a horizontal row of tiles each. Empty results say "Nothing found". A failed search says "Can't search right now".
- **Track rows** gain a plus button before the duration. In edit mode it becomes a remove button, and a drag handle appears at the left. The duration column from the recent fix stays aligned.
- **Now Playing** gains a plus button beside shuffle and repeat.
- **Picker.** A sheet over the screen titled "Add to playlist", with "New playlist" first and the user's playlists as a scrolling list of name rows with small covers. Tapping outside closes it.
- **Detail screen.** On an editable playlist an Edit button sits beside Play and shuffle. In edit mode Play and shuffle hide; Rename, Delete and Done show instead.
- **Delete confirmation.** A centered dialog: "Delete <name>?" with Cancel and Delete. Delete is red.
- **Reorder.** Pressing a drag handle lifts the row; moving it shows where it will land; letting go sends `MoveTrack`. The list auto-scrolls near the top and bottom edges.

## Error handling

| Case | Behavior |
|---|---|
| Offline when an edit is tapped | Notice "No internet right now", no change |
| Spotify refuses an edit (403, 400, 5xx) | Undo on screen, notice "Couldn't save that", warning in the log |
| Network drops mid-edit | Undo, notice "No internet right now" |
| 401 on a write | One fresh-token retry, as for reads, then treated as refused |
| Snapshot conflict (playlist changed elsewhere) | Spotify applies or rejects it; on reject, undo and reload the track list |
| Search fails | "Can't search right now" in the results area; typing again retries |
| `me` not loaded yet | No Edit buttons; picker offers only "New playlist" |

## Testing

- **App core:** keyboard text editing and targets; search debounce, empty query, stale results dropped; each edit's immediate change, success, and undo on failure; duplicate add; offline refusal; picker filtering by owner; create placeholder replaced by the real URI; delete leaving edit mode and the grid.
- **Web API:** each endpoint with the fake HTTP layer, including the `items` 404 fallback, the create fallback to `/users/{me}/playlists`, snapshot updates, and the 401 retry on writes.
- **Library service:** cache rewrites after each edit kind; edits not deduplicated.
- **Fake source:** search and every edit, so `cargo run -p muzak-player -- --fake` exercises the whole feature.
- **Manual:** `probe --write` against a real account, then the README's Mac checklist extended with a Search and Playlists section.

## Risks

- Spotify's development-mode rules for developer apps may limit writes or search. `probe --write` finds this before PR 2's UI is built on it.
- Drag-to-reorder inside a scrolling list on the Pi's touchscreen may feel poor. If so, fall back to up and down buttons in edit mode; the core action `MoveTrack` stays the same.
- Memory: search results and the keyboard add little, well under 5MB, inside the existing budget.
