# ADR 0004: librqbit 9.0.1 behind an engine-neutral port

Status: accepted (2026-10-15). Evidence: [Phase 0 spike results](../research/phase-0-spike-results.md).

## Context

Phase 3 needs a BitTorrent engine for seeding releases and, later, metadata-first learner import and verified streaming. The [dependency feasibility report](../research/dependency-feasibility.md) selected librqbit, and the local-only spike confirmed:

- librqbit interoperates with Zeron's deterministic torrents;
- it serves only verified bytes;
- its default storage cannot seed read-only release directories;
- one of its UDP sockets is created even when trackers are disabled.

AGENTS.md requires engine types to stay behind a capability port and every engine capability to have a cited API and an integration test.

## Decision

1. **`torrent-port`** defines the application-facing contract: `InfoHash`, `NetworkProfile`, `SeedRequest`, `TransferState`, `TransferStats`, `EngineError` (including `UnsupportedCapability`), and the `TorrentEngine` trait. It has no engine dependency. Capabilities are added to it one slice at a time, each with an integration test. The first capability is seeding (`seed`, `stop`, `transfer`, `transfers`, `listen_address`).
2. **`torrent-librqbit`** is the only crate that depends on librqbit, pinned `=9.0.1` with `default-features = false, features = ["rust-tls"]`: no HTTP API, no HTTP API client, no native TLS.
   - API references: [`Session::new_with_opts`](https://docs.rs/librqbit/9.0.1/librqbit/struct.Session.html), `Session::add_torrent` with `AddTorrentOptions::storage_factory`, `Session::delete(_, false)`, `ManagedTorrent::stats`, and the `storage::{StorageFactory, TorrentStorage}` traits.
3. **Explicit network profiles.** The adapter sets every relevant `SessionOptions` field; librqbit defaults (DHT on) are never relied on. UPnP, persistence, and fast-resume are off in all profiles, and the listener is TCP-only.

   | Profile | Listener | DHT | Trackers | Local discovery |
   |---|---|---|---|---|
   | `Loopback` | `127.0.0.1` | off | off | off |
   | `LocalNetwork` | all interfaces | off | the torrent's own | on |
   | `Internet` | all interfaces | on, no DHT persistence | the torrent's own | on |

   `Loopback` additionally binds every socket to `lo` on Linux (`SO_BINDTODEVICE`), which also confines the always-created UDP tracker socket. All automated tests use `Loopback` and never touch public trackers or the DHT.
4. **Release seeding is read-only.** Seeding goes through `ReadOnlyContent`, a storage factory that:
   - opens each torrent file read-only after checking that it is a regular file with the declared length;
   - serves positional reads;
   - refuses every write, removal, and length change.

   Missing or wrong-length files are reported as `ContentMismatch` before anything is served. After librqbit's initial piece check, content that is not completely verified is paused and reported as `Failed`; it is never served or repaired.

## Consequences

- Release immutability (ADR 0003) holds while seeding, at the cost of a small adapter-owned storage implementation (Unix `read_exact_at`, Windows `seek_read`).
- librqbit's DHT, UPnP, LSD, and uTP code is compiled in even when disabled; the dependency adds about 130 crates.
- Outside Linux, `Loopback` relies on the loopback listener alone, so the idle UDP tracker socket stays bound to all interfaces until upstream skips it when trackers are disabled.
- Engine upgrades are confined to `torrent-librqbit` and must keep its integration tests green.
