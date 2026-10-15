# Phase 3: trackers and tracker lists for new releases

Status: complete. Publishers follow published tracker lists and can add trackers of their own. Every release published afterwards carries them in its torrent and magnet link. Only the Internet profile announces to trackers.

## Delivered

- **`publisher-application` (new `trackers` module):**
  - **Tracker lists:** `tracker_lists` / `set_tracker_lists` store the HTTPS URLs of published lists, up to 10.
    - Until the publisher edits them, the defaults are ngosang/trackerslist's [`trackers_best.txt`](https://cdn.jsdelivr.net/gh/ngosang/trackerslist@master/trackers_best.txt) and trackerslist.com's [`best.txt`](https://cf.trackerslist.com/best.txt).
    - Invalid input is `invalid_input` on field `trackerLists`.
  - **List cache:** `store_tracker_list` caches each list's last successful download separately, keeping only usable trackers, so one failing list never wipes another. `tracker_list_statuses` reports each list's tracker count and download time.
  - **Additional trackers:** `trackers` / `set_trackers` hold trackers added by hand.
    - `normalize_trackers` trims, drops blank lines, and de-duplicates in order.
    - It rejects anything other than an absolute `http`, `https`, or `udp` URL with a host, URLs over 512 bytes or containing spaces, more than 100 URLs, and lists too long for a magnet link (field `trackers`).
  - **Effective trackers:** `effective_trackers` and `distribution_options` put the publisher's own trackers first, then each followed list's cached download in order. `merge_tracker_lists` skips unsupported schemes such as `wss`, removes duplicates, and stops at 100 trackers or a 6 KiB magnet budget, well inside the Learner's 8 KiB magnet limit.
  - Publishing reads only the cache and never waits on the network. Each tracker is its own tier, so clients announce to all of them.
  - `TorrentReceipt::tracker_count` reports how many trackers a prepared release carries.
- **`torrent-metainfo`:** `is_tracker_url` exposes the builder's own tracker check, so the setting and the torrent writer cannot disagree. The info hash is unchanged by trackers (ADR 0002).
- **`torrent-librqbit` (ADR 0004 decision 3, amended):** `LocalNetwork` now disables trackers, as `Loopback` already did. Only `Internet` announces to trackers listed in a torrent or a magnet.
- **Admin:**
  - **Commands:** `set_trackers`, `set_tracker_lists`, and `refresh_tracker_lists`. `get_distribution` adds `trackers`, `trackerLists` (URL, tracker count, download time), and `effectiveTrackerCount`.
  - **Downloads:** lists are fetched in Rust with `reqwest` (rustls; already compiled in through librqbit). Fetches are HTTPS only, with a `Zeron-Publisher/<version>` user agent, a 15 s timeout, at most 3 redirects, and a 256 KiB body limit. A list that fails keeps its previous download.
  - **Startup refresh:** under the Internet profile, the Admin refreshes lists older than a day in the background. Other profiles fetch only when Refresh is pressed.
  - Publishing reads the stored list instead of publishing trackerless.
  - **Distribution page, Trackers section:**
    - a "Tracker list URLs" box, prefilled with the defaults, showing each list's tracker count and when it was downloaded, plus a Refresh lists button that reports per-list failures;
    - a separate "Additional tracker URLs" box;
    - the number of trackers new releases will carry;
    - notes that only new releases are affected and that a public tracker shows downloaders' IP addresses to anyone with the magnet.
    Saving the list URLs downloads them straight away.
  - The publish panel shows the release's tracker count, or "None: learners find it through local discovery or the DHT".
- **Publisher CLI:** `prepare` and `publish` use the same stored list, keeping parity with the Admin.
- **Learner:** no new behavior. The Internet description now mentions trackers alongside the DHT.

## Acceptance evidence

- **Application tests:** list URLs default to the two lists, are validated as HTTPS, and are de-duplicated. Each list is cached separately. Hand-added trackers come first, and lists no longer followed stop contributing. Merging respects order, skips `wss` and malformed lines, and stops at 100 trackers or the magnet budget, and a merged list always saves. Additional trackers default to empty; input is trimmed, de-duplicated, and kept in order; one tier per tracker; six kinds of invalid input are rejected without changing the stored list; the list can be cleared. Trackers change the magnet but not the info hash, and the receipt counts them.
- **Adapter tests:** only `Internet` enables trackers and the DHT. The opt-in local-discovery test resolves a magnet listing 70 trackers in the same time as a bare magnet (2.07 s both), under the Local network profile.
- **Admin fetch tests:** plain-HTTP lists are refused. An opt-in (`--ignored`) test downloads both real default lists: 20 and 70 usable trackers.
- **Vitest:** list status with download age, saving an added list URL (which triggers a download), a per-list refresh failure, the total tracker count, saving one URL per line, the privacy note, a rejected list's message, and the publish panel's tracker line, both with and without trackers.
- **Real windows** (release builds, fresh app data, a 24 MB dummy lecture, and a logging HTTP tracker on `127.0.0.1:6969`):
  - **Publishing:** the Admin saved `http://127.0.0.1:6969/announce`, prepared and published the draft, and the magnet carried `&tr=http%3A%2F%2F127.0.0.1%3A6969%2Fannounce`.
  - **Local network:** seeding produced no announces in 20 s.
  - **Internet:** after switching profiles and restarting, the Admin announced (`event=started left=0`) as soon as seeding was restored.
  - **Learner** (Internet profile): it announced while resolving the magnet, got the Admin back from the tracker ("returned 1 peer"), and imported the course in 3.0 s.
- **Real window, tracker lists** (release build, fresh data, Local network profile):
  - The list box was prefilled with both defaults, each "not downloaded yet".
  - Refresh lists showed 20 and 70 trackers, "downloaded just now". New releases would carry 70 trackers after removing duplicates; adding `http://127.0.0.1:6969/announce` by hand made it 71.
  - The published magnet was 3,839 bytes with 71 trackers, the hand-added one first, and the publish panel showed 71.
  - Pasted into a fresh Learner on the Local network profile, it imported in 3.4 s, against 2.7 s for the same magnet without trackers.
- All repository gates pass.

## Current limitations

- **Public visibility:** anyone with a release's magnet can list its downloaders' IP addresses on a public tracker. Courses are not encrypted, so a public magnet means a public course. Paid or private courses need encryption and access control, which are not built yet.
- **Existing releases** keep the trackers they were published with, because releases are immutable.
- **Local-network trackers** are ignored under the Local network profile too.
- **Tracker status** (reachable, failing) is not shown; librqbit only logs it.
- **List contents are trusted only as URLs.** Lists are third-party files; anything that is not a valid tracker URL is dropped, but which trackers they name is up to their maintainers.
- **Reachability:** trackers only help peers find each other. A seeder behind a router still needs an open port (`phase-3-peer-reachability.md`).
