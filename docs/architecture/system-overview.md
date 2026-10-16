# System overview

## Scope and quality attributes

Zeron is a local-first course publishing and learning system: one Tauri 2 desktop app with two areas, side by side ([ADR 0012](../decisions/0012-single-desktop-app.md)):

- **Library** imports local folders, `.torrent` files, and magnet links; streams verified media; persists learning progress and annotations; and optionally shares courses.
- **Publish** builds course drafts, validates and packages immutable releases, creates torrents, and manages seeding.

One Network setting covers both. A private torrent engine downloads and never uploads, and a public engine on the one fixed port serves shared courses and seeds releases. Neither area requires a cloud service. An optional publisher catalog is a later component. Paid courses ([ADR 0017](../decisions/0017-paid-courses-and-licenses.md), roadmap Phase 11) add an optional license issuer that only paid courses contact; free courses never do. There is no headless seed server; the user removed it from the roadmap. The most important quality attributes are offline operation, integrity, recoverability, bounded resource use, explicit network behavior, and preservation of learner-owned data.

## Context and trust boundaries

```text
untrusted course files / torrent peers / trackers
                    |
                    v
        adapters + validation boundary
                    |
      application use cases and policies
             /                  \
   immutable releases      mutable local data
   course packages         drafts, notes, progress
             \                  /
              Tauri command boundary
                       |
                React applications
```

React never receives arbitrary filesystem access, signing keys, database handles, or librqbit objects. Tauri commands translate versioned request/response DTOs into application calls; they contain no business rules. Course paths in manifests are untrusted, relative package paths. OS paths remain backend-only.

## Initial monorepo shape

Create components only when a phase needs them:

```text
apps/
  desktop/               React UI (Library, Publish) + thin Tauri composition root
crates/
  course-format/         shared manifest/release types, schema validation
  course-access/         paid-lesson encryption: content keys, chunk sealing, decrypting reader
  learner-identity/      the learner key: keychain seed, derived Ed25519/X25519 keys, backups
  learning-domain/       annotations, progress, stable identity rules
  torrent-engine/        engine-neutral port plus librqbit adapter
  course-publisher/      draft validation, packaging, torrent creation
  learning-storage/      SQLite repositories and migrations
  learning-application/  learner/admin use cases and orchestration
  observability/         redaction and tracing policy
packages/
  ui/                    shared presentational components when reuse is real
tools/
  course-cli/            packaging/validation automation
tests/fixtures/           deterministic torrents and redistributable media
```

`content-indexer`, `distribution-manager`, and `publisher-catalog` are introduced only in their roadmap phases. Library and Publish share Rust libraries and visual primitives; they keep separate state stores and API modules. `data-import` moves the former separate apps' data in once ([ADR 0011](../decisions/0011-importing-the-separate-apps-data.md)).

## Dependency direction

```text
course-format <---------------- course-publisher
      ^                                 ^
      |                                 |
learning-domain                 learning-application
      ^                           ^     ^
      |                           |     |
learning-storage ----------------+     +--- torrent-engine (port)
                                            ^
                                            |
                                      librqbit adapter

Tauri apps ----------> learning-application (composition only)
```

Rules:

1. Domain crates do not depend on Tauri, React, SQLite, HTTP, or librqbit.
2. `course-format` owns wire schemas but no persistence or network behavior.
3. `torrent-engine` exposes application capabilities and explicit unsupported results; it does not leak engine types.
4. Storage implements repositories defined at the use-case boundary.
5. Cross-crate cycles are forbidden and checked in CI.

## Runtime lifecycle

Startup is a visible readiness sequence: configure tracing with secret redaction; open and migrate SQLite; validate storage roots; restore application records; start librqbit; reconcile engine state and files; start a loopback media gateway on an ephemeral port; then expose readiness. Partial failures produce recoverable degraded states rather than fabricated data.

Shutdown stops new work, cancels playback and indexing, drains bounded writes, persists checkpoints, stops the gateway, asks the torrent session to shut down, and applies a finite deadline. Recovery on next start is authoritative; the process never waits indefinitely.

## State separation

- Published package bytes are immutable and addressed by torrent identity.
- Drafts are mutable publisher data and never share a writable directory with a release.
- Notes, drawings, anchors, progress, and preferences are learner-owned SQLite records, never torrent payload.
- Torrent transfer state, resource availability, and playback state are independent state machines.
- A completed piece is playable only after the engine reports hash verification.

## Data and IPC

SQLite uses WAL mode where platform/filesystem testing permits, foreign keys, transactional migrations, and FTS5. Long-running jobs return stable job IDs and emit throttled, versioned events; UI reconnects by querying a snapshot, so events are hints rather than the source of truth. Cancellation tokens belong to application jobs and are propagated into torrent waits, HTTP responses, and indexing.

## Architectural decisions

| Decision | Choice | Reason |
|---|---|---|
| Desktop topology | One Tauri app with Library and Publish ([ADR 0012](../decisions/0012-single-desktop-app.md)) | One network setting, one public port, and one install for people who both learn and publish |
| Course distribution | Multi-file BitTorrent v1 first | Broadest initial compatibility and per-resource layout |
| Streaming transport | Authenticated loopback HTTP Range gateway | Native media elements understand ranges and backpressure |
| Persistence | SQLite + FTS5 | Transactional offline storage and local search |
| Identity | UUID/ULID-style opaque stable IDs plus SemVer course versions | Names and paths remain editable |
| Engine integration | Capability-based adapter around pinned librqbit 9.x | 9.0.1 verifies the core path while isolating missing/unstable controls |
| Publication | Staged, verified, immutable release directories | Prevents partially published or mutated torrents |

The current evidence supports librqbit's magnet resolution, file selection, persistence, seeding, v1 creation, and verified-piece `AsyncRead + AsyncSeek` stream. It does not support product-level arbitrary piece weights, verified-range snapshots, or configurable lookahead as stable public APIs. Its creator also needs a deterministic, error-preserving wrapper/extension before production publishing. These are architecture constraints, not deferred implementation guesses; see the cited [dependency feasibility report](../research/dependency-feasibility.md).

## Phase 0 gates

Implementation may begin only after approval of these documents. Before committing to librqbit streaming, a throwaway spike must prove manifest-only retrieval, verified byte-range reads, priority changes, seeking, cancellation, resume, and seeding from a locally controlled swarm on all target OS WebViews. A failed spike returns to architectural review; unsupported APIs must not be invented.
