# Phase 3: per-course sharing for learners

Status: complete. Learners share nothing until they switch sharing on for a specific torrent course. A shared course is served by a second, upload-enabled engine session, and its metrics are measured on this computer only. The user chose this design on 2026-10-15 over deferring it or offering one app-wide switch.

## Delivered

- **Storage (learner schema v4):**
  - `course_torrent_sources.sharing` is `0` or `1`, defaults to off, and is migrated from v3 inside a transaction.
  - `set_course_sharing` changes only torrent-sourced courses.
  - Summaries and torrent sources carry the flag.
  - Every import replaces the source row, so a re-imported course starts unshared.
- **`torrent-metainfo::info_hash`:** the SHA-1 of the raw `info` bytes, so the application can route a fetch before any engine sees it.
- **`TransferRouter<E>`** (implements the engine port):
  - Holds a private engine (uploads off) and an optional sharing engine (uploads on).
  - Resolution always uses the private engine. Fetches go to the engine matching the torrent's choice, and the latest fetch per torrent is recorded.
  - `set_shared` stops an active transfer and starts it again in the other engine with the same selection. That engine re-checks the data on disk instead of downloading it. A failed move is undone.
  - Fetches, stops, and moves are serialized, so a transfer is never in both engines.
  - Without a sharing engine, sharing is refused and every course stays private.
- **`CourseSharing<E>`:**
  - `load` applies stored choices before transfers are restored.
  - `set` moves the transfer first and then stores the choice; it reverts the move if storing fails.
  - `statuses` returns each torrent course's choice with its current transfer.
  - Errors: `NotTorrentCourse`, `Unavailable`, `Library`, and `Engine`.
- **Learner backend:**
  - Two engines share the stored network profile: a private one (state `engine/`, uploads off) and a sharing one (state `engine-sharing/`, uploads on). If the sharing engine can't start, sharing is unavailable; if the private one can't start, torrent import is unavailable.
  - Stored choices load before restore, and both engines shut down on exit.
  - New commands `get_course_sharing` and `set_course_sharing` return the choice, availability, network profile, transfer state, connections, bytes uploaded, and upload rate.
  - `readiness` reports `sharing`, and course summaries carry `sharing`.
  - Engine failures cross IPC only as a fixed `sharing_failed` message, because their details can contain paths.
- **Learner UI:**
  - Torrent course pages show "Share with other learners" with an accessible switch, off by default.
  - The description names the audience from the network profile ("on your local network", "on your local network or over the internet", "on this computer") and says that peers can see this computer's IP address. It also says only `course.json` is downloaded so far.
  - While sharing is on, the card shows connections, bytes uploaded, and upload rate, labelled as measured on this computer and not a download count. It refreshes every 2 s.
  - The card shows when sharing is unavailable, why a choice failed, and when shared data no longer matches the torrent.
  - Library cards show a "Sharing" badge.

## Acceptance evidence

- **Storage test:** sharing starts off; it can be turned on and appears in sources and summaries; unknown and local courses are refused; a local re-import clears it.
- **Real-engine loopback test (`only_courses_the_learner_shares_are_served_and_the_choice_survives_restart`):**
  - A course imported through the router stays in the private engine, and a probe given only the private engine's address gets 0 bytes.
  - After `set(true)`, with the seeder already gone, the transfer is in the sharing engine and re-checked from disk. A probe gets the complete `course.json`, and the status reports the choice and at least that many uploaded bytes.
  - After a restart with fresh engines, `load` plus `restore` put the transfer back in the sharing engine.
  - `set(false)` moves it back and is stored. An unknown course is `NotTorrentCourse`.
  - Passed 3 of 3 extra runs.
- **Router test:** without a sharing engine, sharing is refused and turning it off still succeeds.
- **`torrent-metainfo` test:** `info_hash` matches the builder's info hash and rejects input without `info` or that is not bencode.
- **Learner command test:** status mapping (inactive, ready, connections, bytes, rate, profile name); engine errors containing paths become the fixed `sharing_failed` message.
- **Vitest (5 new tests):**
  - off by default with no metrics;
  - turning sharing on shows the audience, the IP notice, local metrics, and refreshes the library;
  - the internet audience and the failed-check notice;
  - the unavailable state disables the switch;
  - a failed choice is explained and the switch stays off.
- **Real windows** (WebDriver, Admin seeding, rebuilt Learner, existing Learner data):
  - The schema 3 database migrated to v4 on start with sharing off.
  - A loopback probe that knew only the Learner's two peer ports got 0 bytes.
  - After switching sharing on in the UI, the same probe got all 949 bytes of `course.json`. The card showed 16.4 kB uploaded (one 16 KiB piece) and the library showed the "Sharing" badge.
  - After a Learner restart the switch was still on, and the probe again got 949 bytes.
  - After switching off, the probe got 0 bytes, and the database stored `sharing = 0`.
- All repository gates pass.

## Current limitations

- Only `course.json` (and the piece it shares with the first resource) is downloaded, so that is all a learner can share until lesson media download exists.
- "Connections" counts connections, not computers. One peer reachable at several local addresses (loopback, LAN, container bridges) is counted once per address; on the test host the Admin appeared as 6.
- Upload counters restart when a course moves between engines or the app restarts.
- Two engine sessions mean two listening ports. There is still no learner UI for the network profile, and a profile change applies at the next launch.
- Moving a course re-checks its data. That is instant for `course.json`, but for large downloaded courses it will take time.
