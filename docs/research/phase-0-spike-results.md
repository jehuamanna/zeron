# Phase 0 spike results: librqbit 9.0.1, local-only

Date: 2026-10-15 · Platform: Linux x86_64 (headless) · Code: `spikes/librqbit-local/` (throwaway, excluded from the workspace)

## Question

Can librqbit 9.0.1, configured for a locally controlled swarm, distribute **Zeron's own published releases** with the behaviour the [distribution architecture](../architecture/distribution.md) requires? Specifically: metadata-first import, manifest-only retrieval, reading a lesson before it completes, seeking into absent data, bounded cancellation, seed metrics, restart reuse, and seeding without mutating the immutable release.

## Setup

- Dependency: `librqbit = { version = "=9.0.1", default-features = false, features = ["rust-tls"] }`. The HTTP API, HTTP API client, and native TLS are not compiled in.
- Fixture: a real release produced by Zeron's pipeline (`PublisherDrafts` → `PublishJobs::run` → `activate`). It has six lessons, including a 24 MiB "lesson 4" video, `course.json`, a deterministic v1 torrent (16 KiB pieces), and read-only release files.
- Sessions: two `Session`s, one seeding and one learning, each with:
  - `dht: None`, `disable_trackers: true`, `disable_local_service_discovery: true`, `persistence: None`, `fastresume: false`, `ipv4_only: true`;
  - a TCP-only listener on `127.0.0.1:0` with UPnP disabled;
  - the seeder's address passed to the learner as `initial_peers`.
- Reproduce: `cd spikes/librqbit-local && cargo run`. Optionally set `SPIKE_BIND_DEVICE=lo` (see finding 3). Run it under `strace -f -e trace=socket,bind,listen,connect` to audit sockets.

## Results

| Scenario | Result |
|---|---|
| librqbit parses Zeron's `.torrent` | Pass. librqbit's computed v1 info hash equals Zeron's. This is the interoperability evidence ADR 0002 deferred. |
| Seed directly from the read-only release (default storage) | **Fails.** `overwrite=false`: "error creating a new file … File exists". `overwrite=true`: "error opening … in read/write mode: Permission denied". The default filesystem storage always opens files writable. |
| Seed through a read-only `StorageFactory` (≈80 lines) | Pass. librqbit's initial check verified all 26.7 MB, the release served the learner, and afterwards every release file was still read-only and byte-identical. |
| Metadata-first import via magnet plus explicit local peer (`list_only`) | Pass. Seven files and a matching info hash in about 40–85 ms. |
| Manifest-only download (`only_files = [course.json]`) | Pass. Exactly one 16 KiB piece was fetched; `course.json` passed `course_format::parse_and_validate`. The neighbouring file received only its shared boundary-piece bytes. |
| Select lesson 4 and read 64 KiB from its middle before completion | Pass. About 45 ms; bytes matched the source; only 98 KB of 25 MB had been downloaded at that moment. |
| Bounded cancellation (pause, then read absent data) | Pass. The read stayed pending (no unverified bytes were served) until the caller's 2 s timeout, and dropping the stream returned immediately. |
| Seed metrics | Pass. `uploaded_bytes` and live upload speed are available from `ManagedTorrent::stats()`. |
| Selected-files completion | Pass. Unselected files hold only boundary-piece bytes. |
| Restart reuse with no peers (new session, same output folder, `overwrite: true`) | Pass. Selected files were re-verified from disk in 20–90 ms without any network. |

## Findings and recommendations

1. **Release immutability needs a read-only storage adapter.** librqbit's default storage cannot seed ADR 0003's read-only release directories. A custom `StorageFactory` and `TorrentStorage` solves this:
   - open each torrent file read-only and check its length at creation;
   - serve `pread_exact` with positional reads;
   - refuse `pwrite_all`, `remove_file`, and `remove_directory_if_empty`;
   - accept `ensure_file_length` only when the length already matches.

   The Phase 3 engine adapter should own this type. Windows needs `FileExt::seek_read` instead of the Unix `read_exact_at`.
2. **Learner downloads use the default storage with `overwrite: true` for resume.** This is only for the learner's own output folder, which is the learner's mutable cache, never a release.
3. **"Local only" needs more than session flags.**
   - `strace` confirmed every `connect`/`sendto` went to `127.0.0.1`, and both TCP listeners bound `127.0.0.1` only.
   - However, `Session::new_with_opts` always creates the UDP tracker client socket bound to `[::]:0` (all interfaces, ephemeral port), even with `disable_trackers: true`.
   - On Linux, `bind_device_name: Some("lo")` applies `SO_BINDTODEVICE` to all 14 sockets without privileges, and the full scenario still passes. CI and test sessions should use it.
   - Other platforms need either an upstream change (skip the UDP tracker client when trackers are disabled) or an accepted, documented idle UDP socket.
4. **DHT, UPnP, LSD, and uTP code is compiled in unconditionally;** it is disabled only at runtime. librqbit adds 133 crates (name/version pairs) over the current workspace lockfile, including `ring`/`rustls` for the tracker HTTP client. Production defaults must set every network feature explicitly rather than rely on librqbit's defaults; DHT, for example, defaults to enabled.
5. **`FileStream` is a sound integrity boundary for the gateway:** it never returned unverified data, and seeking ahead made the scheduler fetch the needed pieces first.

## Not covered (still required before Phase 3 completion)

- HTML media playback through the authenticated gateway in each packaged WebView (Linux WebKitGTK, macOS WKWebView, Windows WebView2): time to first frame, seek behaviour, two concurrent readers.
- Peer interruption mid-transfer and reconnection; real-world latency; uTP.
- Lookahead and priority tuning (the fixed internal 32 MiB window) under playback.
- macOS and Windows runs of everything above, including the read-only adapter's positional reads.
- Long-running seeding, session persistence across restarts, and bandwidth limits.

## Recommendation

Proceed to Phase 3 with librqbit 9.0.1 pinned (`default-features = false`, `rust-tls`) behind the engine-neutral port, adopting:

1. the read-only release storage adapter for seeding;
2. explicit network defaults, with no reliance on librqbit defaults;
3. `bind_device_name = "lo"` for CI and local tests.

Record the adoption in an ADR when the adapter lands. Treat the uncovered WebView and cross-OS items above as acceptance criteria for the Phase 3 playback slices.
