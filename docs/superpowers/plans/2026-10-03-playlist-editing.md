# Playlist Editing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add songs to playlists, create playlists named on screen, and on playlists the user owns: remove, reorder, rename and delete.

**Architecture:** Optimistic edits in the pure core with an undo record per edit; a new `LibraryRequest::Edit` carried out by `LibrarySource::apply`; after success the core reloads the affected track list and Playlists section, which rewrites the cache from Spotify's answer. PR 2 and PR 3 of the spec ship as one PR because they share all of the plumbing.

**Tech Stack:** Rust, Slint 1.18, reqwest.

**Spec:** `docs/superpowers/specs/2026-10-03-search-and-playlists-design.md`. Deviation: `probe --write` is dropped; the user wants simpler setup scripts instead of more probe work.

---

### Task 1: Model and Web API writes
- [ ] `Collection.owner_id`, `Collection.snapshot_id` (done in the WIP commit). `PlaylistObj.owner.id` and `snapshot_id` fill them.
- [ ] `Http::send_json(method, url, token, body)`; empty bodies decode to `Value::Null`. `WebApi::get` becomes `request(method, url, body)` with the shared 401 retry.
- [ ] `PlaylistEdit { Add, Create, Remove, Move, Rename, Delete }` and `EditOutcome { Done, Created(Collection) }` in `model.rs`. `LibrarySource::apply(edit)`.
- [ ] Endpoints per the spec table. Move sends `insert_before = to + 1` when moving down.
- [ ] `SCOPES` gains `playlist-modify-private,playlist-modify-public` in both crates.
- [ ] Tests: each edit's method, URL and body via `FakeHttp`; create then add; delete falls back to `/followers` on 404.

### Task 2: Fake catalog edits
- [ ] `FakeCatalog` keeps its data behind a `Mutex` with accessor methods, so `FakeSource::apply` can edit what the fake player plays.
- [ ] Test: create, add, move, remove, rename, delete on the fake catalog.

### Task 3: Library service
- [ ] `LibraryRequest::Edit { id, edit }` → `apply` → `LibraryUpdate::EditDone { id, outcome }` or `EditFailed { id, reason }`. Edits skip in-flight de-duplication.

### Task 4: Core
- [ ] State: `picker: Option<String>`, `text_entry: Option<TextEntry>`, `editing: Option<String>`, `confirm_delete: Option<String>`. Notices become `Clone` and gain `CouldntSave`, `AddedTo(String)`, `AlreadyIn(String)`.
- [ ] Actions: `OpenPicker(track_uri)`, `ClosePicker`, `PickPlaylist(uri)`, `NewPlaylist`, `KeyboardDone`, `CancelText`, `EditPlaylist(uri)`, `FinishEditing`, `RemoveTrack(index)`, `MoveTrack { from, to }`, `RenamePlaylist`, `AskDelete`, `ConfirmDelete`, `CancelDelete`.
- [ ] Keys go to `text_entry` when it is open, else to search.
- [ ] Each edit: refuse with "No internet right now" when offline; save undo; change state; emit `Edit`. `EditDone` drops undo, swaps a created placeholder for the real playlist, and reloads. `EditFailed` restores, notifies, and reloads.
- [ ] Only playlists with `owner_id == account.id` are editable or offered in the picker.
- [ ] Tests for every action, success and failure.

### Task 5: View and UI
- [ ] `TrackRowView.uri`; `DetailView.editable`, `DetailView.editing`; `PickerView`; `TextEntryView`; `confirm_delete`; keyboard shown for search or text entry; banner texts.
- [ ] Track rows: plus button; in edit mode a drag handle and a remove button. Drag moves by whole rows.
- [ ] Detail: Edit button for owned playlists; edit mode shows Rename, Delete and Done.
- [ ] Picker sheet, text entry dialog with the keyboard, delete confirmation, plus button on Now Playing and on search song rows.

### Task 6: Verify and ship
- [ ] fmt, clippy, tests; fake mode end to end; README checklist; re-run `auth-web` for the new scopes; PR.
