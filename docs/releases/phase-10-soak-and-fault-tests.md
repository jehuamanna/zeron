# Phase 10: soak, performance, and fault tests

Status: complete. The media gateway is now tested under sustained load and against clients that stall or vanish, through real loopback connections. This note also lists the fault tests elsewhere in the suite, so the coverage is in one place.

## Delivered

- **`crates/media-gateway/tests/soak.rs`:**
  - **`hundreds_of_concurrent_ranges_stay_byte_exact`:** eight workers, two per lesson across four registered resources, make 400 random ranges of up to 256 KiB from a 1 MiB file. Every body is compared byte for byte, and a whole-file `GET` follows.
  - **`the_stream_limit_holds_and_vanished_clients_free_their_slots`:**
    - With a limit of two streams, two clients that never read a 64 MiB body hold both slots, and a third request gets 503.
    - When they disconnect, the slots come back.
    - After 50 clients that read 4 KiB and vanish, two concurrent requests are served again, so nothing leaks.
  - **`sustained_random_ranges_for_a_minute`** (`--ignored`; `ZERON_SOAK_SECONDS` sets the length) runs sequential random ranges of up to 1 MiB from an 8 MiB file, each checked.
- **Gateway behaviour the first soak run surfaced:** the gateway keeps the two newest bodies per resource, and a third cancels the oldest, as when a player seeks. The soak's first version fired eight concurrent ranges at one resource and saw cut-short bodies. That is the designed behaviour, already covered by `newer_requests_cancel_the_oldest_waiting_body_and_revoke_cancels_all`. The soak now uses the gateway as the app does.

## Measurements (2026-10-16, Linux x86_64, debug build)

- 400 concurrent ranges, 45 MiB in total, took about 30 ms (about 1.5 GB/s) through loopback HTTP.
- The long soak served 28,291 ranges in 10 s, sequentially, with no failures.

## Fault coverage across the suite

| Fault | Where it is tested |
|---|---|
| Crash during release activation, file deletion, or recovery | `publisher-application` `tests/releases.rs` (startup recovery, half-done deletions) |
| Crash during the old-app data import | `data-import` tests (each step, then a second run) |
| Crash while a restore is switched in | `data-backup` `an_interrupted_switch_finishes_on_the_next_start` |
| A cancelled or failed preparation, and a source changed during staging | `publisher-application` `tests/publish_jobs.rs` |
| Low disk space during downloads and preparation | `learning-application` `low_disk_space_pauses_downloads_and_recovery_resumes_only_those`, `publish_jobs` `preparation_refuses_to_fill_the_disk` |
| A peer that never sends metadata or `course.json` | `learning-application` `tests/torrent_import.rs` (metadata timeout, stalled manifest, cancellation) |
| A seek into absent data, and a restart while streaming | `phase-4-controlled-swarm-milestone.md` tests |
| Tampered packages, torrents, signatures, catalogs, and backups | `releases.rs`, `signing.rs`, `signed_import.rs`, `data-backup`, and the robustness and fuzz tests |
| Stalled and vanished gateway clients | `media-gateway` `tests/soak.rs` |

## Acceptance evidence

- The two bounded tests run in every gate in under a second. The long soak passed for 10 s locally.
- All repository gates pass.

## Current limitations

- Measurements come from a debug build on one Linux machine; release-build numbers per platform belong to the release checklist.
- There is no soak over real torrent swarms. The controlled-swarm milestone tests cover correctness, not duration.
