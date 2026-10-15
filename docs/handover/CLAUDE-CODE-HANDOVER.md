# Claude Code handover — Zeron

Handover date: 2026-10-15

Repository: `git@github.com:jehuamanna/zeron.git`

Branch: `main`

Last completed implementation commit: `930b4d5 feat: persist publisher drafts`

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

## Active incomplete slice: Admin Desktop foundation

Work began after `930b4d5` and is intentionally uncommitted. Inspect it before editing:

- `Cargo.toml` adds `apps/admin-desktop/src-tauri` as a workspace member.
- Root `package.json` changes build/test/typecheck scripts to target every app.
- `apps/admin-desktop/` contains the initial React/Vite package, publisher API types, Courses/Builder UI, and two Vitest tests.
- The Admin Tauri backend does not exist yet. Because the workspace references it, current Cargo commands will fail until it is created or the partial member is temporarily removed.
- `pnpm-lock.yaml` has not yet been refreshed for the new workspace package.
- No Admin release note has been written and none of this partial slice has been committed.

Do not discard the partial UI. Complete the vertical slice:

1. Add `apps/admin-desktop/src-tauri/Cargo.toml`, `build.rs`, `src/main.rs`, `src/lib.rs`, `src/commands.rs`, `tauri.conf.json`, and the minimal capability file.
2. Compose `publisher-storage::PublisherStorage` in Tauri setup using an app-data `publisher.sqlite3` file behind `Arc<Mutex<_>>`.
3. Implement thin `list_drafts`, `create_draft`, and `get_draft` commands.
4. Generate secure opaque draft/course IDs in Rust. A new draft starts at revision zero with `0.1.0`, “Untitled course”, “Local publisher”, `en`, and no modules.
5. Return path-free DTOs. `get_draft` may expose course metadata and module/lesson/resource counts, but never `DraftResource.source_path`.
6. Use a distinct Tauri identifier (`app.zeron.publisher`), development port `1421`, strict CSP, and independently packaged product name such as “Zeron Publisher”. The existing Learner icon may be referenced or copied only if packaging accepts it.
7. Refresh the pnpm lockfile, run both app test/type/build gates and all Rust gates, then add a Phase 2 release note.
8. Commit and push the completed slice while excluding `AGENTS.md`.

Likely Rust dependencies for Admin Tauri are `course-publisher`, `publisher-storage`, `getrandom`, `serde`, `tauri`, and `tauri-build`. Keep exact versions through workspace pins.

## Important module boundaries

- `course-format`: manifest wire contract and semantic validation.
- `course-publisher`: SQLite/Tauri-independent draft and publishing policy.
- `publisher-storage`: mutable Admin draft persistence and compare-and-swap saves.
- `learning-application`: learner import/library use cases.
- `learning-storage`: learner persistence.
- `media-gateway`: authenticated loopback HTTP delivery; no Tauri or torrent dependency.
- Desktop apps: composition and path-free DTO translation only.

Published package bytes are immutable. Drafts and releases must never share writable state. Learner-owned data stays outside package directories. Long-running work must be cancellable and bounded.

## Roadmap position

Phase 1 is functionally established, including complete local-file playback. Phase 2 is active.

After completing the Admin Desktop foundation, continue Phase 2 in small slices:

1. Admin draft application service and autosave edits with conflict reporting.
2. Module/lesson/resource CRUD and accessible reordering.
3. Backend-only source selection plus path-free resource DTOs.
4. Deterministic package staging and canonical `course.json` generation.
5. Media inspection warnings without silent transcoding.
6. Deterministic BitTorrent v1 metainfo generation and independent verification.
7. Atomic immutable release activation and release history.
8. Local seeding management through the shared torrent adapter.

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
git diff -- Cargo.toml package.json apps/admin-desktop
git log --oneline -12
```

Completion of the active slice means the partial Admin files compile, every repository gate is green, the release note is accurate, a focused commit is pushed, and `git status --short` shows only the user-owned `AGENTS.md` modification.
