# Phase 3: tracker list for new releases

Status: complete. Publishers can list trackers, and every release published afterwards carries them in its torrent and magnet link. Only the Internet profile announces to trackers.

## Delivered

- **`publisher-application`:**
  - `PublisherDrafts::trackers`, `set_trackers`, and `distribution_options` store a publisher-wide list. `normalize_trackers` trims, drops blank lines, and de-duplicates while keeping the order.
  - It rejects anything other than an absolute `http`, `https`, or `udp` URL with a host, URLs longer than 512 bytes or containing spaces, and more than 16 URLs. The error is `invalid_input` on field `trackers`.
  - Each tracker becomes its own tier, so clients announce to every tracker instead of stopping at the first that answers.
  - `TorrentReceipt::tracker_count` reports how many trackers a prepared release carries.
- **`torrent-metainfo`:** `is_tracker_url` exposes the builder's own tracker check, so the setting and the torrent writer cannot disagree. The info hash is unchanged by trackers (ADR 0002).
- **`torrent-librqbit` (ADR 0004 decision 3, amended):** `LocalNetwork` now disables trackers, as `Loopback` already did. Only `Internet` announces to trackers listed in a torrent or a magnet.
- **Admin:**
  - The `set_trackers` command, and `trackers` in `get_distribution`.
  - Publishing reads the stored list instead of publishing trackerless.
  - The Distribution page has a Trackers form: one URL per line, a note that only new releases are affected, and a warning that a public tracker shows downloaders' IP addresses to anyone with the magnet.
  - The publish panel shows the release's tracker count, or "None: learners find it through local discovery or the DHT".
- **Publisher CLI:** `prepare` and `publish` use the same stored list, keeping parity with the Admin.
- **Learner:** no new behavior. The Internet description now mentions trackers alongside the DHT.

## Acceptance evidence

- **Application tests:** settings default to empty; input is trimmed, de-duplicated, and kept in order; one tier per tracker; six kinds of invalid input are rejected without changing the stored list; the list can be cleared. Trackers change the magnet but not the info hash, and the receipt counts them.
- **Adapter test:** only `Internet` enables trackers and the DHT.
- **Vitest:** saving one URL per line, the privacy note, a rejected list's message, and the publish panel's tracker line, both with and without trackers.
- **Real windows** (release builds, fresh app data, a 24 MB dummy lecture, and a logging HTTP tracker on `127.0.0.1:6969`):
  - **Publishing:** the Admin saved `http://127.0.0.1:6969/announce`, prepared and published the draft, and the magnet carried `&tr=http%3A%2F%2F127.0.0.1%3A6969%2Fannounce`.
  - **Local network:** seeding produced no announces in 20 s.
  - **Internet:** after switching profiles and restarting, the Admin announced (`event=started left=0`) as soon as seeding was restored.
  - **Learner** (Internet profile): it announced while resolving the magnet, got the Admin back from the tracker ("returned 1 peer"), and imported the course in 3.0 s.
- All repository gates pass.

## Current limitations

- **Public visibility:** anyone with a release's magnet can list its downloaders' IP addresses on a public tracker. Courses are not encrypted, so a public magnet means a public course. Paid or private courses need encryption and access control, which are not built yet.
- **Existing releases** keep the trackers they were published with, because releases are immutable.
- **Local-network trackers** are ignored under the Local network profile too.
- **Tracker status** (reachable, failing) is not shown; librqbit only logs it.
- **Reachability:** trackers only help peers find each other. A seeder behind a router still needs an open port (`phase-3-peer-reachability.md`).
