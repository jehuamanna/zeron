# Phase 3: metadata-first learner torrent import

Status: complete. Learners can add a course from a `.torrent` file or a magnet link. Zeron resolves verified metadata, downloads only `course.json`, validates it against the torrent's file inventory, and adds the course outline to the library. Lesson media is not downloaded in this slice.

## Delivered

- **`torrent-port`:** `NetworkProfile::name` / `from_name` are the stable storage and IPC names. The publisher's `network_profile_name` and `parse_network_profile` now delegate to them.
- **Storage (learner schema v3):**
  - `course_torrent_sources` records the info hash and complete metainfo of torrent-sourced courses. It cascades with `courses`, and a later local-folder import of the same course clears it.
  - A `settings` table stores the network profile.
  - Course summaries carry the torrent info hash.
- **`CourseLibrary`:**
  - `save_torrent_course`, `torrent_course_sources`, `torrent_info_hash`.
  - `network_profile` defaults to **Local network**; `set_network_profile` persists it.
  - `local_resource` and `resource_path` return nothing for torrent-sourced courses, so no unverified torrent bytes can reach the media gateway.
- **`TorrentImports<E: TorrentEngine>`** runs one import at a time; a second is refused as busy.
  - Stages: `Queued → ResolvingMetadata → FetchingManifest → Validating → Imported`, ending in `Failed` or `Cancelled` otherwise.
  - Bounded: 120 s for metadata and 60 s without `course.json` progress. Cancellable at every await. Observable through sequenced snapshots and a notify callback.
  - Validation before anything is saved:
    - The torrent inventory must be safe relative paths with `course.json` present.
    - The manifest must fit the import limits and pass `course-format` validation.
    - Every manifest resource must exist in the torrent with the declared length, and the torrent must contain no other files.
  - Content lives in app-data `courses/<info hash>/`. On failure or cancel, the transfer stops and the directory is removed if this import created it and no library course uses it.
  - Failures carry stable codes without paths: `metadata_timeout`, `invalid_source`, `engine_failed`, `unsafe_inventory`, `not_a_course`, `limit_exceeded`, `manifest_stalled`, `manifest_invalid`, `inventory_mismatch`, `storage_failed`.
  - `restore` re-activates the `course.json` selection of every torrent-sourced course after a restart, offline from the stored metainfo.
- **Learner backend:**
  - Startup launches `LibrqbitEngine` with the stored profile; engine state lives in app-data `engine/`. A failed start is non-fatal: local courses keep working, and the detailed error goes only to the process log because it can contain paths.
  - Torrent courses are restored in the background, and the engine shuts down on exit.
  - New commands:
    - `choose_torrent_file_and_import`: a native file dialog in Rust; the `.torrent` read is capped at 4 MiB.
    - `import_magnet`: the link must start with `magnet:?` and is limited to 8 KiB.
    - `get_torrent_import` and `cancel_torrent_import`.
  - Progress is sent as `torrent-import` events.
  - `readiness` reports whether torrent import is available.
  - Course summary and detail DTOs carry `source` (`local`/`torrent`) and `torrentInfoHash`.
  - `register_media` refuses torrent courses with `media_not_downloaded`.
- **Learner UI:**
  - An "Add a course from a torrent" panel offers "Open .torrent file" and "Paste magnet link". It shows a four-step progress (find metadata, download `course.json` with verified bytes, check structure, add to library). Cancel is available while running; failures show their reason with Retry ("Choose file again" for files); cancelled imports state that nothing was added.
  - The library refreshes when an import completes.
  - Course cards show "Local · verified" or "Torrent · outline only".
  - A torrent course opens its real outline with a notice that only `course.json` is verified. Videos show "Video · not downloaded" instead of a Play button.
  - While readiness loads, the actions are disabled without claiming the engine failed.

## Acceptance evidence

- **Storage test:** torrent sources are recorded, listed for restore, reported in summaries, and cleared by a local re-import.
- **Library tests:** the network profile defaults to Local network and persists across reopen; local courses have no torrent info hash.
- **Real-engine loopback tests (`learning-application/tests/torrent_import.rs`):**
  - A course published by Zeron's pipeline and seeded by a loopback librqbit engine is imported from a bare magnet with only the seeder's address. The stages run exactly in order.
  - The stored course reports its info hash. `course.json` is byte-identical, while the largest resource stays below 1.5 MB verified, so only the manifest was fetched. Playback paths are withheld.
  - After the learner engine shuts down, a new engine and service restore the course's transfer.
  - A torrent built over the release plus one extra file ends `Failed` with `inventory_mismatch`, and nothing is added.
- **Scripted-engine tests:**
  - Unreachable metadata times out as `metadata_timeout`. Cancel during resolution ends `Cancelled`. A second concurrent import is refused as busy.
  - A torrent without `course.json` is `not_a_course`; a traversal path is `unsafe_inventory`; a manifest that stops progressing is `manifest_stalled`. The library stays empty in every case.
- **`torrent-port` unit test:** network-profile names round-trip.
- **Learner command tests:**
  - Snapshots map to path-free DTOs with stage strings and manifest byte progress.
  - Magnet links are trimmed, must use the `magnet:?` scheme, and are bounded.
  - `.torrent` files over 4 MiB are refused without being read whole. Unreadable files report no path.
- **Vitest (12 tests):**
  - A magnet import follows events through each stage and ignores stale sequences; completion refreshes the library.
  - Cancel works, and a cancelled file import offers "Choose file again".
  - A failure explains itself, and Retry resends the same magnet.
  - Start errors are shown.
  - The unavailable engine disables actions, and pending readiness doesn't claim a failure.
  - Torrent courses withhold playback; library cards show their source.
- **Manual verification:**
  - **Real debug binary** (isolated `XDG_DATA_HOME`): it created learner schema 3 with `course_torrent_sources` and `settings`, plus an `engine/` directory. `ss` showed a TCP peer listener on all interfaces, UDP 6771 (local discovery), librqbit's idle UDP tracker socket, and the loopback-only media gateway. There was no DHT socket, matching the default Local-network profile.
  - **Browser pane** (Vite plus a fake IPC bridge):
    - A magnet import showed the progress steps and `course.json` byte progress, then "Added … to your library". The new card showed "Torrent · outline only".
    - The course opened with the outline-only notice and "Video · not downloaded".
    - A failing import showed its reason; Retry resent the same magnet, and Cancel ended in "Import cancelled".
    - There was no horizontal overflow at 375 px.
- All repository gates pass.

## Current limitations

- Only `course.json` is downloaded. Lesson media download, verified streaming through the gateway, and playback of torrent courses come in the following Phase 3/4 slices.
- There is no learner UI to change the network profile; the default Local-network profile applies unless the setting is changed in storage.
- No learner seeding, upload controls, or dashboard metrics yet. When this slice shipped, the learner engine also served its verified `course.json` pieces to peers. `phase-3-learner-uploads-off.md` turned learner uploads off.
- **Corrected after release:** a later real-window check (see `phase-3-magnet-local-discovery.md`) found that magnet imports did not work through local discovery alone. librqbit's metadata-only resolve never announced, so the import timed out unless the seeder's five-minute announcement happened to arrive in time. `.torrent` imports were unaffected. The follow-up slice fixes this.
- Only one import runs at a time. Finished import snapshots are kept in memory (the last 16) and are lost on restart; the imported course itself is durable.
