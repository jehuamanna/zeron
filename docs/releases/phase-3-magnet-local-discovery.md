# Phase 3: magnet import through local discovery

Status: complete. A learner can import a course from a bare magnet link when the only way to find the seeder is local service discovery on the same network. That is the default Local-network profile with no tracker in the link.

## Problem

Driving the real Admin and Learner windows found that magnet imports timed out after 120 s, although the Admin was seeding on the same machine. A passive listener on the discovery multicast group (239.192.152.143:6771) showed why: the seeder announced at startup, but the learner sent nothing while resolving.

In librqbit 9.0.1, a list-only add passes `announce = !list_only` (`session.rs`). With announce off, local discovery only listens. The learner therefore had to wait for the seeder's periodic announcement, which comes every five minutes (`librqbit-lsd`). `.torrent` imports were unaffected: their metadata resolves offline, and the following fetch is a normal add that announces.

## Delivered

- **`torrent-librqbit` magnet resolution** (ADR 0004, decision 5):
  - A magnet is added live with no file selected, so the learner announces itself and the seeder answers.
  - It uses `MetadataOnly`, a storage that creates no file and refuses every read and write. No file named by untrusted metadata is created before the application validates the inventory. librqbit's initial check treats the unreadable pieces as missing.
  - Once metadata is verified, the adapter waits up to 2 s for the metadata peers to reconnect, records any that did, then removes the torrent. Removal runs in its own task, so it completes even if the import is cancelled at that moment.
  - A magnet for a torrent the engine already holds is answered from that torrent's metadata without the network. Before, librqbit resolved it from peers first, so an engine holding the torrent but unable to reach any peer failed.
  - `.torrent` bytes are still resolved offline.
- **`torrent-port`:** `ResolvedTorrent::peers` lists the peers connected when metadata arrived, as a hint for the following fetch. It is empty for offline resolution.
- **`TorrentImports`:** the `course.json` fetch dials the caller's peers plus the metadata peers, without duplicates.
- **How the fetch finds the seeder again.** The fetch re-adds the torrent, and the seeder ignores an address it has already seen. The seeder also answers discovery announcements at most once per wall-clock second per torrent. Two mechanisms cover this:
  - If metadata peers reconnect within the wait, they are returned as hints and the fetch dials them directly. This usually happens on loopback with a supplied peer.
  - If none reconnect, the full 2 s wait moves the fetch's announcement past the seeder's reply limit, so the seeder answers. This is what happens with discovery alone: a seeder doesn't stay connected to a learner that needs nothing, so the hint is empty.

## Acceptance evidence

- **Failing first:** the new opt-in adapter test `magnets_resolve_through_local_service_discovery_alone` uses two Local-network engines and a bare magnet, after the seeder's start-up announcement has passed. Before the fix, it found no seeder within 60 s. After the fix, metadata arrives in about 50 ms; the call returns after about 2.1 s, including the peer wait, with an empty hint.
- **Real windows, before the fix:** with WebDriver (`tauri-driver` + `WebKitWebDriver`), the Learner's magnet import ended "No peer provided the course metadata within 120 seconds" on two runs. A `.torrent` import of the same release succeeded.
- **Real windows, after the fix:** with the same flow and the Admin seeding:
  - The Learner's magnet import found metadata at +0.3 s, fetched `course.json` at +2.3 s, and added the course at +3.0 s.
  - The course opened with its outline and the outline-only notice, and was still in the library after the Learner restarted.
  - In an intermediate build that removed the resolve torrent without the 2 s peer wait or peer hints, metadata resolved but the `course.json` fetch stalled for 60 s. The seeder ignores an address it has already seen and rate-limits its replies.
- **New loopback adapter test:** after a magnet resolve, no transfer remains and no torrent file exists anywhere under the learner engine's state. Any peer hint is the seeder's address; under load the seeder may not reconnect within the wait, and the hint is then empty. An engine already seeding the torrent resolves its magnet with no peers and keeps seeding.
- **New opt-in import test:** `a_magnet_imports_through_local_service_discovery_alone` imports a course end to end through `TorrentImports` with a bare magnet. It passed on 5 of 5 consecutive runs, each in about 2.2 s.
- **Unit test:** metadata peers are appended after caller peers without duplicates.
- All repository gates pass. The opt-in tests pass with `cargo test -- --ignored`.

## Current limitations

- A magnet resolve takes about 2 s longer when its metadata peers don't stay connected, which is the usual case with discovery alone.
- With discovery alone, the hand-off depends on librqbit's one-second reply rate limit staying below the 2 s wait. If another learner announces the same torrent in the same second as the fetch, that fetch waits for the seeder's next announcement.
- The opt-in tests multicast on every interface of the machine, so they are not part of the default test run.
- The real-window flow (WebDriver for both apps, `xdotool` for the native file dialog, a scratch publisher for the fixture release) is a manual procedure, not a committed harness.
- Discovery between two different machines on a real LAN is still unverified; both apps ran on one host.
