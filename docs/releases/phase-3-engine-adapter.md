# Phase 3: torrent engine port and librqbit adapter

Status: complete for seeding immutable content through an engine-neutral port (see ADR 0004). Admin seeding and learner import build on this in the following slices.

## Delivered

- `torrent-port`: an engine-neutral contract.
  - `InfoHash` is validated and normalized to lowercase hex.
  - `NetworkProfile` is `Loopback`, `LocalNetwork`, or `Internet`.
  - `SeedRequest` holds torrent bytes and a read-only content root.
  - `TransferStats` covers state (`Checking`/`Seeding`/`Failed`), verified and total bytes, uploaded bytes, upload rate, connected peers, and failure.
  - `EngineError` includes `UnsupportedCapability`, `InvalidTorrent`, `ContentMismatch`, `AlreadyActive`, `NotActive`, and `Engine`.
  - The `TorrentEngine` trait offers `seed`, `stop`, `transfer`, `transfers`, and `listen_address`.
- `torrent-librqbit`: the only crate depending on librqbit (pinned `=9.0.1`, no default features, `rust-tls`).
  - `LibrqbitEngine::start(EngineConfig { profile, state_directory, listen_port })` builds every session option explicitly per profile. `Loopback` also binds all sockets to `lo` on Linux. `shutdown` stops the session without deleting content.
  - Seeding uses the read-only `ReadOnlyContent` storage. Missing or wrong-length files become `ContentMismatch`; content that fails librqbit's initial check is paused and reported `Failed` with "the content on disk no longer matches the torrent; nothing is being served".
  - `stop` removes the torrent without touching files.
- Workspace pins `librqbit =9.0.1` and `anyhow =1.0.104`.

### Learner capabilities (resolve and fetch)

- `TorrentEngine::resolve(ResolveRequest { source, initial_peers })` returns verified metadata without downloading content. Sources are `.torrent` bytes (resolved offline) or a `magnet:` URI with a v1 info hash (fetched from peers). The result is `ResolvedTorrent`: info hash, name, files with engine index, untrusted path and length, and the complete metainfo for later restore. Malformed torrents and magnets without an info hash are `InvalidTorrent`.
- `TorrentEngine::fetch(FetchRequest { metainfo, content_root, files, initial_peers })` downloads only the selected files into a caller-owned writable directory and reuses verified data on restart. Repeating the request for an active torrent replaces its selection.
- `TransferStats` adds `downloaded_bytes` and per-file `file_verified_bytes`; `TransferState` adds `Downloading`.

## Acceptance evidence

- `torrent-port` unit test: info-hash normalization and rejection of malformed values.
- `torrent-librqbit` integration tests (loopback only, using a real release published by Zeron's pipeline):
  - The engine listens on loopback and seeds the read-only release in place. Its state reaches `Seeding` with verified bytes equal to total bytes.
  - A second seed of the same torrent is `AlreadyActive`.
  - An independent loopback librqbit learner, given only the seeder's address, downloads every file byte-identically.
  - The seeder reports uploads at least equal to the content size.
  - `stop` works, a second `stop` is `NotActive`, and every release file is still read-only and byte-identical afterwards.
  - A writable copy with one flipped byte ends `Failed` with fewer verified bytes than total, and the file is not modified.
  - A copy with a missing file is rejected as `ContentMismatch` before anything is served; non-torrent bytes are `InvalidTorrent`.
- Loopback learner test: the adapter resolves a published release offline from its `.torrent` and from a bare magnet using only the seeder's address, with identical file tables (`course.json` first). A magnet without an info hash is rejected. Fetching only `course.json` yields a byte-identical manifest while the largest file stays below half verified. Widening the selection completes that file.
- All repository gates pass.

## Current limitations

- Verified streaming (`FileStream` behind the gateway) is a later capability of the port.
- Dropping a `resolve` future abandons resolution, but librqbit may finish its in-flight peer handshakes in the background.
- `LocalNetwork` and `Internet` profiles are configured but not exercised by automated tests, because CI must not use public networks; they need manual LAN and internet checks when exposed in the UI.
- Outside Linux, the `Loopback` profile cannot confine librqbit's idle UDP tracker socket.
- No engine session persistence; seeding intent will be restored by the application layer.
