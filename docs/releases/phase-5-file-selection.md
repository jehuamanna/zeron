# Phase 5: select which lesson files download

Status: complete. This is the first Phase 5 slice: an engine capability for choosing which files of an active torrent download in full. Download commitments in the Learner build on it next.

## Delivered

- **`torrent-port`:** `TorrentEngine::select_files(info_hash, files)` replaces the set of files that download in full. Verified pieces of deselected files stay on disk, and streaming still reads any file on demand. The default implementation returns `UnsupportedCapability("file selection")`, so engines without selection say so.
- **`torrent-librqbit`** (ADR 0004 decision 9):
  - `select_files` calls `Session::update_only_files` (librqbit 9.0.1, `src/session.rs`) after a bounded wait for the initial check.
  - It refuses an empty set, out-of-range indices (`InvalidTorrent`), and inactive torrents (`NotActive`).
- **`learning-application`:** `TransferRouter::select_files` delegates to the engine holding the course and updates the remembered fetch request. A course moved between the private and sharing engines therefore keeps its selection.

## Acceptance evidence

- **Adapter integration test** `selected_files_download_in_full_and_stay_after_deselection`, on loopback:
  - After a `course.json`-only fetch, selecting one of three 6 MB lessons downloads it in full with no reader open, even though the uploads-off learner had dropped its peers.
  - The other two lessons stay below half verified, so only boundary pieces are shared.
  - Deselecting keeps the lesson fully verified. Inactive, empty, and out-of-range selections are refused.
- **Router test:** a selection is passed to the active engine and carried along when sharing moves the course in either direction. An unknown torrent is `NotActive`.
- All repository gates pass.

## Current limitations

- With librqbit persistence off, selections do not survive a restart by themselves. The commitments slice re-applies them on restore.
- There is no per-file priority beyond selection, and no pause or resume yet.
