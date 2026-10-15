# Phase 5: speed limits

Status: complete. Learners can cap download and upload speed, and publishers can cap seeding upload speed. Caps apply immediately and survive restarts.

## Delivered

- **`torrent-port`:**
  - `RateLimits { download_bytes_per_second, upload_bytes_per_second }`, where `None` means unlimited.
  - `from_kib_per_second` rejects caps below 16 KiB/s and values that overflow. `from_settings` and `to_settings` handle the stored KiB/s strings; damaged values fall back to unlimited.
  - `TorrentEngine::set_rate_limits` is synchronous and returns `UnsupportedCapability` by default.
- **`torrent-librqbit`** (ADR 0004 decision 11): sets the session's live limits, `Session::ratelimits.set_download_bps` and `set_upload_bps` (librqbit 9.0.1, `src/limits.rs`). Every torrent in the session shares them.
- **Learner:**
  - `CourseLibrary::rate_limits` and `set_rate_limits` use the `download_limit_kib` and `upload_limit_kib` settings.
  - `TransferRouter` applies the caps to both the private and sharing engines.
  - Stored caps are applied when the engines start.
  - The `set_rate_limits { downloadKib, uploadKib }` command validates, stores, and applies the caps live; errors are `invalid_input`. `get_network_settings` adds `downloadLimitKib` and `uploadLimitKib`.
  - The Network page has a "Speed limits" form.
- **Admin:**
  - `PublisherDrafts::upload_limit` and `set_upload_limit` store an upload cap only, since the Admin only seeds.
  - The cap is applied when the engine starts.
  - The `set_upload_limit { uploadKib }` command reports errors as `invalid_input` with `field: "uploadLimitKib"`, and `get_distribution` adds `uploadLimitKib`.
  - The Distribution page has an "Upload limit" form.

## Acceptance evidence

- **Adapter test** `rate_limits_cap_uploads_and_downloads_live`, three 2 MB lessons on loopback:
  - With a 256 KiB/s seeder upload cap, one lesson took at least 4 s.
  - After the seeder cap was lifted and a 256 KiB/s learner download cap set, the next took at least 4 s.
  - With no caps, the last finished in under 4 s.
  - The whole test ran in about 16 s.
- **Unit tests:** conversion, the minimum cap, overflow, and the settings round trip.
- **Library and Admin persistence tests:** caps default to unlimited, persist across reopen, and clear.
- **Command tests:** validation in both apps.
- **Vitest:** both forms load their values, reject values under 16 KiB/s without calling the backend, treat empty as unlimited, save, and refresh.
- All repository gates pass.

## Current limitations

- Caps are per app and session-wide, not per course.
- The Learner's upload cap only matters for courses the learner shares.
