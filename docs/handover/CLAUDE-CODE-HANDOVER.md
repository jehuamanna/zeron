# Claude Code handover — Zeron

Handover date: 2026-10-15

Repository: `git@github.com:jehuamanna/zeron.git`

Branch: `main`

Last completed implementation slice: Admin Desktop foundation (after `930b4d5 feat: persist publisher drafts`)

The Claude handover itself was added in commit `64293de`.

## Mission

Continue implementing Zeron through the dependency-ordered roadmap without waiting between safe, in-scope slices. For each slice: implement, test, document acceptance evidence and limitations, commit, and push. Pause only for a genuine product decision, unavailable credential/environment, destructive action outside the approved scope, or an architectural blocker supported by evidence.

The product is an offline-first learning platform with independently packaged Learner and Admin Tauri applications. BitTorrent distributes immutable course packages; it does not own learning or publishing domain models.

## Repository state at handover

The following commits are pushed and green:

| Commit | Completed slice |
|---|---|
| `c27a1e5` | Strict version-one course manifest and JSON Schema |
| `428709b` | Safe, bounded local course import and SHA-256 verification |
| `f52c767` | Learner SQLite storage and migrations |
| `b5c8445` | Course library application composition |
| `8298409` | Learner Desktop foundation |
| `cf0485e` | Path-free course navigation and package-location recovery |
| `c97e724` | Authenticated loopback media gateway and complete-file video playback |
| `3ed82a0` | Publisher draft model, validation, ordering, and publishing state machine |
| `930b4d5` | Publisher draft SQLite persistence and optimistic autosave revisions |

The last fully green baseline at `930b4d5` passed Rust formatting, Clippy with warnings denied, and all workspace tests. The media slice additionally passed four React tests, strict TypeScript, and the production Vite build.

## User-owned working-tree change

`AGENTS.md` is modified by the user. Preserve it and exclude it from implementation commits. Its added instruction is to keep implementing and testing all phases continuously.

## Completed after handover

- Admin Desktop foundation (`docs/releases/phase-2-admin-desktop-foundation.md`).
- Draft application service and metadata autosave (`docs/releases/phase-2-draft-application-service.md`). `publisher-application::PublisherDrafts` owns draft use cases; `apps/admin-desktop/src-tauri` composes it against app-data `publisher.sqlite3`. Command bodies are Tauri-independent functions tested against in-memory storage. Command errors carry `code`, plus `currentRevision` for conflicts and `field` for invalid input.
- Module and lesson structure editing (`docs/releases/phase-2-structure-editing.md`). All Builder writes go through one serialized `draftWriter` per editing session (`apps/admin-desktop/src/features/builder/draftWriter.ts`); new editors must submit through it so they share the revision sequence and the conflict banner.
- Backend-only source selection (`docs/releases/phase-2-source-selection.md`). `choose_and_attach_resource` opens the native picker in Rust; `PublisherDrafts::attach_source` canonicalizes and records the path only in the snapshot; resource DTOs expose `fileName` and `byteLength` only.
- Deterministic package staging (`docs/releases/phase-2-package-staging.md`, ADR `docs/decisions/0001-package-staging-layout.md`). `course_publisher::stage_package` plus `publisher_application::stage_draft` / `discard_abandoned_staging`. Backend only; not yet wired into the Admin app.
- Media inspection warnings (`docs/releases/phase-2-media-inspection.md`). `course_publisher::inspect_media` (bounded ISO-BMFF header scan), durations captured on attach, on-demand "Media checks" in `ReadinessPanel`.
- Deterministic BitTorrent v1 metainfo (`docs/releases/phase-2-torrent-metainfo.md`, ADR `docs/decisions/0002-torrent-metainfo-writer.md`). New crate `torrent-metainfo`; `publisher_application::generate_torrent` / `verify_staged_torrent` operate on completed staging attempts.
- Admin release preparation job (`docs/releases/phase-2-publish-preparation-job.md`). `publisher_application::PublishJobs`; Admin commands `start_publish_preparation` / `get_publish_job` / `cancel_publish_job` plus `publish-job` events; `PublishPanel` UI.
- Immutable release activation and history (`docs/releases/phase-2-release-activation.md`, ADR `docs/decisions/0003-release-activation.md`). Publisher DB schema v2 (`releases`), `PublisherDrafts::activate_release` / `recover_releases`, `PublishJobs::activate`, Admin `activate_release` / `list_releases`, Releases list on the Courses page.
- Publisher CLI parity (`docs/releases/phase-2-publisher-cli.md`). `tools/publisher-cli` and `PublisherDrafts::verify_release`. Phase 2 is complete.

Local tooling note: `pnpm` is not on `PATH` in this environment; it is available as `corepack pnpm` from `~/.nvm/versions/node/v22.22.2/bin`. The root scripts invoke nested `pnpm`, so put a `pnpm` shim that execs `corepack pnpm` on `PATH` before running the JS gates.

## Next slice

Phase 3 (local seeding and metadata-first learner torrent import) is next. The user approved a local-only librqbit spike first; it is done. Read `docs/research/phase-0-spike-results.md` before starting. Summary:

- librqbit 9.0.1 (`default-features = false`, `rust-tls`) parses Zeron's torrents with a matching info hash. It resolves metadata from a magnet plus an explicit local peer, downloads only `course.json`, reads lesson 4 mid-file before completion, keeps reads of absent data pending (bounded by the caller's timeout), exposes seed metrics, and reuses verified data after restart without peers.
- Seeding the read-only release directories requires a custom read-only `StorageFactory`; the default storage opens files read/write. The spike's ≈80-line adapter (`spikes/librqbit-local/src/main.rs`) worked and left the release byte-identical.
- `Session::new_with_opts` always binds a UDP tracker socket on `[::]:0`. Use `bind_device_name = "lo"` for CI and local tests on Linux, and set every network option explicitly in production.

The user approved Phase 3 integration. Done: `torrent-port` + `torrent-librqbit` (ADR `docs/decisions/0004-librqbit-engine-adapter.md`, `docs/releases/phase-3-engine-adapter.md`). Admin seeding is done too (`docs/releases/phase-3-admin-seeding.md`): `SeedingService`, schema v3 intents and settings, a Distribution workspace, and a default Local-network profile. Next: learner `.torrent`/magnet import with manifest-first validation. The uncovered spike items (WebView playback per OS, peer interruption, lookahead) are acceptance criteria for the Phase 3/4 playback slices. Confirm with the user before adding the dependency to the workspace if the scope differs from this plan.

When testing UI in the browser pane, Tauri IPC can be faked by assigning `window.__TAURI_INTERNALS__ = { invoke, transformCallback }` after the page loads, then using the app (the initial list query will already have failed).

## Important module boundaries

- `course-format`: manifest wire contract and semantic validation.
- `course-publisher`: SQLite/Tauri-independent draft and publishing policy.
- `publisher-storage`: mutable Admin draft persistence and compare-and-swap saves.
- `publisher-application`: Admin draft use cases (create, autosave, validate) and input bounds.
- `learning-application`: learner import/library use cases.
- `learning-storage`: learner persistence.
- `media-gateway`: authenticated loopback HTTP delivery; no Tauri or torrent dependency.
- Desktop apps: composition and path-free DTO translation only.

Published package bytes are immutable. Drafts and releases must never share writable state. Learner-owned data stays outside package directories. Long-running work must be cancellable and bounded.

## Roadmap position

Phase 1 is functionally established, including complete local-file playback. Phase 2 is complete (Admin builder, reproducible publishing, immutable releases, CLI parity). Phase 3 is next and needs a dependency decision.

Continue Phase 2 in small slices:

1. ~~Admin draft application service and autosave edits with conflict reporting.~~ Done.
2. ~~Module/lesson CRUD and accessible reordering.~~ Done; resource CRUD moves to item 3.
3. ~~Backend-only source selection plus path-free resource DTOs.~~ Done.
4. ~~Deterministic package staging and canonical `course.json` generation.~~ Done (backend).
5. ~~Media inspection warnings without silent transcoding.~~ Done.
6. ~~Deterministic BitTorrent v1 metainfo generation and independent verification.~~ Done (backend).
7. ~~Atomic immutable release activation and release history.~~ Done.
8. Local seeding management through the shared torrent adapter (roadmap places this in Phase 3 with the librqbit adapter).

Then proceed through Phases 3–10 in [docs/roadmap.md](../roadmap.md). Do not jump to librqbit integration until the current Phase 2 slice has a green committed boundary.

## Known constraints and review hotspots

- Course/torrent/media paths are untrusted. Resolve and validate them in Rust; never accept arbitrary paths from React.
- The loopback gateway uses an IPv4 loopback ephemeral port, a per-process random secret, opaque handles, strict single ranges, 64 KiB chunks, eight concurrent streams, body-frame timeout, and explicit app-exit shutdown.
- The current gateway serves complete verified local files only. Torrent-backed verified sparse reads remain later work.
- `librqbit` should remain behind an engine-neutral capability port. Do not expose its types or enable its full HTTP management server.
- Draft source paths currently serialize inside backend-only draft snapshots. Admin IPC must map them away.
- Schema and IPC changes require Rust types, TypeScript contracts, fixtures/tests, compatibility notes, and round-trip evidence together.

## Git discipline

The repository is configured with `origin` pointing to GitHub. The user already authorized creating and pushing the repository and requested continuous implementation. Normal focused commits and pushes to `main` are in scope. Force pushes, destructive resets, cleaning the working tree, or incorporating the user-owned `AGENTS.md` edit are outside the handover scope.

Before the first edit, run:

```bash
git status --short
git diff
git log --oneline -12
```

Completion of any slice means every repository gate is green, the release note is accurate, a focused commit is pushed, and `git status --short` shows only the user-owned `AGENTS.md` modification.
