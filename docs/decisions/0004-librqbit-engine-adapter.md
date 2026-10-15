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
3. **Explicit network profiles.** The adapter sets every relevant `SessionOptions` field; librqbit defaults (DHT on) are never relied on. Persistence and fast-resume are off in all profiles, and the listener is TCP-only. UPnP is off unless the user opts in under `Internet` (decision 8).

   | Profile | Listener | DHT | Trackers | Local discovery |
   |---|---|---|---|---|
   | `Loopback` | `127.0.0.1` | off | off | off |
   | `LocalNetwork` | all interfaces | off | off (amended 2026-10-16) | on |
   | `Internet` | all interfaces | on, no DHT persistence | the torrent's own | on |

   Trackers are usually public servers, so `LocalNetwork` ignores every tracker a torrent or magnet lists. That keeps its promise that nothing is announced to the internet. A tracker on the local network is ignored too.

   `Loopback` additionally binds every socket to `lo` on Linux (`SO_BINDTODEVICE`), which also confines the always-created UDP tracker socket. Automated tests use `Loopback` and never touch public trackers or the DHT. Tests that exercise local service discovery use `LocalNetwork`, multicast on the machine's own networks, and are opt-in (`--ignored`).
4. **Release seeding is read-only.** Seeding goes through `ReadOnlyContent`, a storage factory that:
   - opens each torrent file read-only after checking that it is a regular file with the declared length;
   - serves positional reads;
   - refuses every write, removal, and length change.

   Missing or wrong-length files are reported as `ContentMismatch` before anything is served. After librqbit's initial piece check, content that is not completely verified is paused and reported as `Failed`; it is never served or repaired.

5. **Magnet resolution announces, but stores nothing.** librqbit's list-only add never announces to local service discovery or the DHT (`announce = !list_only` in `session.rs`), so a learner would only hear a seeder's periodic announcement, every five minutes. A magnet is therefore resolved with a live add that selects no file and uses `MetadataOnly`, a storage that creates no file and refuses every read and write. Once metadata is verified, the adapter waits up to 2 s, longer than discovery's one-second reply rate limit, for metadata peers to reconnect. It records them as `ResolvedTorrent::peers` and removes the torrent; that removal runs to completion even if the caller stops waiting. A torrent already active in the engine is answered from its own metadata without the network. `.torrent` bytes are still resolved offline with list-only.

6. **Uploading is an explicit engine setting.** `EngineConfig::allow_uploads` maps to librqbit's session-wide `disable_upload` (Cargo feature `disable-upload`). With uploads off, the engine advertises no pieces, rejects piece requests, and refuses `seed`. librqbit has no per-torrent switch, so per-course sharing uses a separate session: the Learner runs a private engine and a sharing engine, and `learning_application::TransferRouter` moves a course's transfer between them.

7. **Streaming wraps `FileStream` and bounds it.** `open_file` returns librqbit's verified-piece `FileStream` behind the port's `VerifiedFile`, without a parallel piece scheduler. The adapter limits readers to 4 per engine, because each stream holds one of the session's 8 blocking permits that piece I/O also needs. While a read waits, it re-requests peer reconnection every 3 s by opening and dropping a short-lived stream, which works around librqbit 9.0.1 missing a peer whose disconnect was in progress when the stream opened.

8. **Peer reachability is an explicit, opt-in user choice** (amended 2026-10-16). `EngineConfig::reachability` carries `torrent_port::PeerReachability`: a fixed port of 1024–65535, or a new ephemeral port on each start, plus a UPnP choice. `PeerReachability::for_profile` decides what applies, so a stored choice can never widen a narrower profile:

   | Profile | Fixed port | UPnP forwarding |
   |---|---|---|
   | `Loopback` | ignored | never |
   | `LocalNetwork` | applies | never |
   | `Internet` | applies | when chosen |

   Forwarding uses librqbit's own `enable_upnp_port_forwarding`, which maps the TCP listen port with a 60 s lease that it renews while the session runs. The mapping therefore lapses within about a minute of shutdown, and no stale router state outlives the app. A fixed port is off by default. Forwarding was opt-in at first; since 2026-10-16 it is **on by default** (amended below). The Learner's fixed port applies to its sharing engine only.

   **Amendment, 2026-10-16: forwarding on by default, on every engine.** A real two-machine test failed: the publisher's router offered no UPnP, NAT-PMP, or PCP, and the learner's port was never mapped. Neither side could accept a connection, so the learner searched for metadata forever. Common BitTorrent clients avoid this by mapping ports by default on both sides. Only one side of a connection must be reachable, and a seeder that can upload dials out to every peer it discovers; librqbit's DHT re-queries at least every 60 s. Zeron now does the same:
   - a stored setting without an explicit "off" means forwarding on;
   - the Learner's private download engine also maps its ephemeral port, so a seeder behind a router without port mapping can still reach the learner.

   Forwarding still applies only under `Internet`, and both apps can turn it off. The user asked for this on 2026-10-16 ("make it just utorrent work").

   This supersedes the earlier "UPnP off in all profiles" rule. Reaching seeders across the internet behind home routers requires an inbound port, and the user chose an opt-in toggle over manual forwarding alone. The risk is that any LAN device can make the same UPnP request, and the router opens a port to the internet. That is why it is opt-in, scoped to `Internet`, and explained in both apps.

   **Amendment, 2026-10-16: Zeron maps the ports itself** ([ADR 0013](0013-zeron-port-mapper.md)). librqbit 9.0.1's UPnP maps TCP only, has no NAT-PMP or PCP, and reports nothing back. The desktop app therefore always starts its engines with `port_forwarding: false`, and the `port-mapping` crate opens both engines' ports for TCP and UDP. The policy above is unchanged: forwarding is the user's opt-in choice, on by default, and applies only under `Internet`. The adapter keeps its `enable_upnp_port_forwarding` mapping for other composition roots.

9. **File selection replaces the download set** (added 2026-10-16). `TorrentEngine::select_files(info_hash, files)` maps to `Session::update_only_files` (librqbit 9.0.1, `src/session.rs`). That call re-queues newly selected pieces that are not verified yet and leaves verified pieces of deselected files on disk. The adapter rejects an empty set or out-of-range indices, and waits (bounded) for the initial check first. The port's default implementation returns `UnsupportedCapability`. `TransferRouter` delegates and records the new set, so moving a course between the private and sharing engines keeps it. With persistence off, selections do not survive a restart; the application re-applies them when it restores courses. Integration test: `selected_files_download_in_full_and_stay_after_deselection`.
10. **Pause and resume** (added 2026-10-16). `TorrentEngine::pause` and `resume` map to `Session::pause` and `Session::unpause` (librqbit 9.0.1, `src/session.rs`). Both are idempotent and wait (bounded) for the initial check. A seed paused by the adapter because its content failed verification refuses `resume`, so changed content is never served. `TransferState::Paused` reports user pauses. `TransferRouter` re-pauses a course moved between engines, and the Learner re-applies stored pauses on restore. Integration test: `paused_transfers_download_nothing_until_resumed`.
11. **Speed limits are session-wide and live** (added 2026-10-16). `TorrentEngine::set_rate_limits(RateLimits)` sets `Session::ratelimits` via `Limits::set_download_bps` and `set_upload_bps` (librqbit 9.0.1, `src/limits.rs`), which every torrent in the session shares and which take effect immediately. Caps under 16 KiB/s are rejected so a typo cannot stall transfers. The Learner applies the same caps to its private and sharing engines; the Admin sets an upload cap only. Both apps re-apply stored caps when the engine starts. Integration test: `rate_limits_cap_uploads_and_downloads_live`.

## Consequences

- Release immutability (ADR 0003) holds while seeding, at the cost of a small adapter-owned storage implementation (Unix `read_exact_at`, Windows `seek_read`).
- librqbit's DHT, UPnP, LSD, and uTP code is compiled in even when disabled; the dependency adds about 130 crates.
- Outside Linux, `Loopback` relies on the loopback listener alone, so the idle UDP tracker socket stays bound to all interfaces until upstream skips it when trackers are disabled.
- Engine upgrades are confined to `torrent-librqbit` and must keep its integration tests green.
- librqbit reports UPnP results only in its logs; since ADR 0013 the desktop app uses its own mapper and shows what the router answered. Routers with no port mapping, and carrier-grade NAT, still prevent inbound connections; hole punching is not used.
