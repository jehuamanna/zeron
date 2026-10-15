# Phase 4: lessons play when opened right after startup

Status: complete. A torrent lesson opened in the first moments after the Learner starts now waits for its course torrent, instead of failing with a media error. The fix came from driving the real windows with the user's two course folders. This note also records the playback evidence gathered with those courses for the roadmap's first milestone.

## Problem

At startup the Learner restores each torrent course in the background. librqbit adds the torrent in an `initializing` state while it re-checks the bytes already on disk, and while in that state it refuses to stream ("with_storage_and_file: invalid state: initializing"). The gateway answered `503`, and WebKit reported `MediaError 4` (source not supported).

Opening lesson 3 of the 2.2 GB course 0.4–0.5 s after launch failed in 4 of 5 runs, while opening it 20 s after launch played every time. The UI went on showing "Playing" with no error; the playback state machine (open Phase 4 item 1) will cover that.

## Delivered

- **`torrent-librqbit`:** `open_file` waits for `ManagedTorrent::wait_until_initialized` (librqbit 9.0.1, `src/torrent_state/mod.rs`) before opening a stream. The wait is bounded at 120 s and happens before taking one of the four reader permits. A torrent that is still checking after the bound returns an engine error, and a torrent in librqbit's error state returns that error.
- **`learning-application`:** `open_lesson_stream(engine, info_hash, file_index, StreamWait)` retries `EngineError::NotActive` every 200 ms for up to 30 s. That covers the window before restore re-adds the torrent. Every other error is returned immediately.
- **Learner:** `TorrentMedia` opens lessons through `open_lesson_stream`.

## Acceptance evidence

- **New adapter test** `a_stream_opened_while_the_torrent_is_still_checking_waits_for_it`, failing first:
  - It seeds a 100 MB lesson and opens it immediately. It asserts the engine was still `Checking` at that moment, then reads the first 4 KiB and compares them with the source.
  - Before the fix it failed with exactly the real-window error. After the fix it passes.
- **New application tests:** a lesson opened while the engine reports `NotActive` three times then succeeds; other errors return after one attempt; the wait is bounded.
- **Real windows**, run on 2026-10-16 with the user's courses: Admin and Learner each driven by WebDriver, in isolated data directories, under the Local network profile on one host.
  - **Admin:**
    - **Drafts:** both courses were built with the drop pipeline (`plan_drop` + `apply_drop`), giving 13 lessons and 20 lessons, then prepared and published through the UI.
    - **Preparation:** "Systems Design Interview Questions" (14 files, 2.2 GB, 1,145 pieces of 2 MB) prepared in 62 s. "Systems Design Fundamentals" (21 files, 1.5 GB, 1,547 pieces of 1 MB) prepared in 42 s.
    - Both were seeding within 2 s of the start.
  - **Learner, by magnet:**
    - **Import:** each course imported in 2.6 s.
    - **Playback before completion:** lesson 4 of "Systems Design Fundamentals" was playing 3.0 s after the click, with 48% of it downloaded. Lesson 4 of the other course behaved the same way: 3.0 s, 24% downloaded.
    - **Seek into absent data:** a seek to 75% (1302 s) resumed after 7.1 s, with decoded frames advancing past the seek point.
    - **Restart:** after a Learner restart the lesson still showed 77% downloaded and replayed in 2.1 s, so verified bytes are reused.
  - **The fix itself:** opening a lesson 0.4–0.9 s after launch played in 5 of 5 runs, compared with 1 of 5 before.
- All repository gates pass.

## Findings recorded for later slices

- **Missing GStreamer plugins crash the web process.** When `gst-plugins-good` and `gst-libav` are missing (they were absent on this machine), WebKitGTK's web process aborts (`SIGABRT`, "autoaudiosink not found") as soon as a video plays, and the WebDriver session is lost. The release AppImage bundles GStreamer. Development machines need the packages listed in the handover.
- **Unreliable `readyState` after a seek.** After a seek, WebKitGTK keeps `readyState` at 2 while frames advance and `currentTime` moves at 1×, even once the whole file is local. The player state machine must use events and progress, not `readyState` alone.
- **Errors not shown.** The Learner does not yet show `<video>` errors.

## Current limitations

- The milestone evidence above was gathered by hand in real windows. A committed controlled-swarm harness is still open in Phase 4.
- Not checked on macOS or Windows.
