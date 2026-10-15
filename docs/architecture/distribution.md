# Distribution and streaming architecture

## Capability boundary

The application-facing torrent port is capability-based. It covers adding metainfo or magnets, waiting for metadata, enumerating files, selecting files, observing verified availability, reprioritizing requested regions where supported, pausing/resuming, metrics, seeding, restore, and shutdown. Operations may return `UnsupportedCapability`; callers do not infer support from engine version strings.

The research report at [dependency-feasibility.md](../research/dependency-feasibility.md) is the evidence ledger. Phase 0 tests, not type signatures alone, decide whether librqbit is sufficient.

At the researched 9.0.1 baseline, librqbit `only_files`/`update_only_files` support selection and `FileStream` supplies verified-piece `AsyncRead + AsyncSeek`. Active streams receive an internal fixed 32 MiB seek-following priority window. There is no stable public arbitrary piece-priority, verified-range snapshot, or configurable-lookahead API. The first implementation therefore wraps `FileStream`; it adds only a narrow upstreamable lookahead/telemetry patch if measured tests require it, and never builds a parallel piece scheduler.

## Transfer lifecycle

Application states are `QUEUED`, `RESOLVING_METADATA`, `FETCHING_MANIFEST`, `ALLOCATING_OR_CHECKING`, `DOWNLOADING`, `PAUSED`, `STALLED`, `VERIFYING`, `COMPLETED`, `SEEDING`, `ERROR`, and `REMOVING`. They are derived from user intent plus engine observations, not copied one-for-one from librqbit.

Normal progress is queued → metadata → manifest → checking/downloading → verifying → completed → optional seeding. Pause may be entered from network-active states and resumes to the reconciled prior intent. A no-peer timeout yields stalled, not failed. Recoverable errors retain intent and verified bytes. Removal first stops active streams and the engine job; deleting files is a separate explicitly confirmed transition. On restart, persisted user intent is reconciled with metainfo, engine session state, and actual files before any completion/seeding claim is restored.

## Manifest-first import

1. Add `.torrent` metadata locally or resolve a magnet and verify its info hash.
2. Enumerate normalized torrent files and locate exactly one root `course.json` (a configurable conventional location can follow in a later schema version).
3. Select only manifest pieces, accepting boundary-piece overlap.
4. Wait for verified bytes, enforce a small manifest size limit, then parse and schema-validate.
5. Cross-check every manifest resource path, size, optional digest, and media type against torrent metadata.
6. Persist the course structure and show it. Failure produces an explicit retryable import state; no structure is invented.

## Playback data flow

```text
HTML media element
  -> GET /v1/media/{opaque-id}?token={session-secret} + Range
  -> loopback gateway (resource registry, auth, limits)
  -> playback session / buffer controller
  -> torrent adapter verified-range wait/read
  -> bounded response stream
```

The gateway binds an OS-selected port on `127.0.0.1` and `::1` only where dual-stack behavior is verified. A random per-process secret and opaque resource handle authorize a registered course resource; no request contains an OS path. The URL is short-lived, redacted from logs, excluded from analytics and referrers, and invalid after process restart.

The gateway implements single-range `GET` and `HEAD` first, exact `206`/`Content-Range`/`Content-Length`/`Accept-Ranges`, `416` with `bytes */length`, MIME allowlisting, bounded chunks, request timeouts, concurrency limits, and disconnect cancellation. Multi-range requests may be rejected until deliberately implemented. No wildcard CORS is enabled.

## Buffering and seeking

Each playback session owns a generation counter and cancellation token. A range request maps file-relative offsets to torrent pieces. Missing pieces receive urgent priority if the adapter supports it; the response waits asynchronously for integrity-verified availability. Look-ahead is a bounded, replaceable hint, not a second unbounded download queue.

On seek or lesson switch, increment the generation, cancel obsolete waits/prefetch, persist position, reduce old priorities, and register the new range. Already verified pieces remain cached. Time-to-byte mapping is delegated to the browser/container through HTTP ranges; the application never assumes linear bitrate.

Playback states are `IDLE`, `RESOLVING_FILE`, `PREPARING_STREAM`, `BUFFERING`, `PLAYING`, `PAUSED`, `SEEKING`, `STALLED`, `COMPLETED`, `ERROR`, and `CANCELLED`. Torrent transfer states are separate. UI labels separately report player-buffered ranges, verified cached ranges, file completeness, and overall torrent transfer.

## Local distribution

Admin and learner use the same adapter and persisted session model. Starting a seed validates that all release files still match metadata. Dashboard metrics are explicitly local: current upload rate, bytes uploaded by this engine session/history, currently connected peers, ratio derived from local counters, last activity, and errors. No global download count is presented.

## Optional remote seed server

The later `seed-server` is a Linux-first Rust executable composed from the same engine and release libraries. Default deployment has no public management listener. When enabled, management requires TLS, pinned/verified server identity, scoped revocable credentials, replay-resistant requests, audit logs, and rate limits.

Remote activation is staged: create upload → transfer chunks to an attempt directory → verify manifest and torrent pieces → atomically activate → start seed. Disconnects are resumable, and an incomplete transfer is never seedable. Docker volumes and systemd hardening are deployment concerns, not domain dependencies.

## Catalog and authenticity

The optional catalog stores discovery metadata, not course payload or learner activity. Torrent hashes identify bytes, not publishers. A future signed release envelope will use deterministic/canonical serialization and Ed25519, with a key ID, validity interval, rotation statement, and revocation channel. This is deliberately deferred until its canonicalization and trust model receive a separate ADR.

## Required feasibility spike

The first spike must run a local seeder and learner against a multi-file fixture with `course.json` and a fast-start MP4. It must demonstrate metadata resolution, manifest-only retrieval, selection of lesson four, playback before completion, forward seek into absent data, peer interruption, restart reuse, bounded cancellation, and seed metrics. Packet/file instrumentation must prove that unrelated file pieces were not intentionally selected except shared boundary pieces.
