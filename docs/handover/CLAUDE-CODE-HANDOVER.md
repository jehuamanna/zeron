# Claude Code handover — Zeron

Handover date: 2026-10-16

Repository: `git@github.com:jehuamanna/zeron.git`, branch `main`

Last pushed commit before this handover: `6c9ad4b build: package the desktop apps as AppImages`

This document replaces the 2026-10-15 handover. A fresh Claude Code session on another machine should be able to continue from it alone.

## Mission

Zeron is an offline-first learning platform made of two separately packaged Tauri apps:

- **Learner Desktop:** imports, plays, and shares courses.
- **Admin Desktop (Publisher):** builds, publishes, and seeds courses.

BitTorrent distributes immutable course packages. It does not own the learning or publishing domain models.

Continue through the dependency-ordered roadmap ([docs/roadmap.md](../roadmap.md)) without waiting between safe, in-scope slices. For each slice: implement it, test it, document acceptance evidence and limitations in a release note under `docs/releases/`, commit, and push to `main`. Pause only for:

- a genuine product decision;
- a missing credential or environment;
- a destructive action outside the approved scope;
- an architectural blocker you can show evidence for.

Read first: [AGENTS.md](../../AGENTS.md) (authoritative rules), [CLAUDE.md](../../CLAUDE.md), [system-overview](../architecture/system-overview.md), [distribution](../architecture/distribution.md), [admin-publisher](../architecture/admin-publisher.md), the ADRs in `docs/decisions/` (0001–0005), and [phase-0 spike results](../research/phase-0-spike-results.md).

## Start-of-session checklist

```bash
git status --short          # expect only " M AGENTS.md" (user-owned, never commit it)
git log --oneline -12
git diff
```

`AGENTS.md` carries a user-owned edit, the instruction to keep implementing and testing all phases. Preserve it and keep it out of every commit.

## Environment setup on a new machine

Required:

- stable Rust;
- Node 22 with pnpm 11.25.0 through corepack (`corepack enable pnpm`). If `pnpm` is still not on `PATH`, run `corepack enable --install-directory <dir> pnpm` and prepend `<dir>`;
- Tauri v2 Linux prerequisites: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libsoup-3.0-dev`, `librsvg2-dev`, and build-essential;
- for in-app playback on Linux: `gstreamer1.0-plugins-base/good/bad` and `gstreamer1.0-libav`.

Optional, for real-window checks:

- WebKitWebDriver. Debian and Ubuntu package it as `webkit2gtk-driver`. Arch does not ship it, so on Arch (the current machine) build only that target from the matching `webkitgtk-<version>.tar.xz` from webkitgtk.org. This needs `cmake`, `ninja`, `gperf`, `unifdef`, and `ruby-getoptlong`. Configure with `-DPORT=GTK -DUSE_GTK4=OFF -DENABLE_WEBDRIVER=ON`, run `ninja bin/WebKitWebDriver`, and install the binary to `~/.local/bin`. The build sources live in `~/.cache/zeron-webdriver`;
- `cargo install tauri-driver --version 2.1.0 --locked`.

Native GTK dialogs cannot be driven by WebDriver. Seed fixtures through the application API, for example with a throwaway example program, into an isolated `XDG_DATA_HOME`. Treat dialog completion as a manual check. Drive the session with plain W3C HTTP calls; the element key is `element-6066-11e4-a52e-4f735466cecf`.

On Node 26 corepack is not bundled. Install pnpm into a scratch directory with `npm install --prefix <dir> pnpm@11.25.0` and prepend `<dir>/node_modules/.bin` to `PATH`.

Build an app with `pnpm --filter @zeron/<app> tauri build --debug --no-bundle` so the frontend is embedded (plain `cargo build` debug binaries load the Vite dev URL). Run one tauri-driver per app with distinct `--port` / `--native-port`.

Repository gates. All must pass before every commit:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
pnpm test
pnpm typecheck
pnpm build
git diff --check
```

Tests that multicast or listen on the local network are `#[ignore]`. Run them on purpose with `cargo test -p <crate> --test <file> -- --ignored`.

Packaging: Linux builds are **AppImage only** (user decision 2026-10-16: no `.deb`). Use `pnpm build:appimage` (root) or per app. The Learner AppImage bundles GStreamer (`bundleMediaFramework: true`). Don't run the slow release AppImage build before pushing unless the user asks.

Real-course fixture: the user's 2.3 GB `13-Systems-Design-Interview-Questions/files` folder (13 H.264/AAC MP4s) was on a removable drive of the old machine. Ask the user where it is on this machine. Do not use the 30 GB "Back-To-Back-SWE" course. Treat fixtures as read-only input. Publish them only into a scratch directory, keep them on Loopback or Local network, and never commit anything derived from them.

UI testing in a browser pane: fake Tauri IPC by assigning `window.__TAURI_INTERNALS__ = { invoke, transformCallback }` after load.

## What is done

### Phase 0 — architecture and feasibility: done

Schemas, trust boundaries, crate graph, a local-only librqbit 9.0.1 spike, and a metainfo writer decision (ADR 0002).

### Phase 1 — local course foundation: done

| Commit | Slice |
|---|---|
| `c27a1e5` | Strict v1 course manifest and JSON Schema |
| `428709b` | Safe, bounded local course import with SHA-256 verification |
| `f52c767` | Learner SQLite storage and migrations |
| `b5c8445` | Course library application composition |
| `8298409` | Learner Desktop foundation |
| `cf0485e` | Path-free course navigation and package-location recovery |
| `c97e724` | Authenticated loopback media gateway; complete-file playback |

### Phase 2 — Admin builder and reproducible publishing: done

| Commit | Slice |
|---|---|
| `3ed82a0` / `930b4d5` | Draft model, state machine, SQLite persistence with optimistic revisions |
| `8adb0f8` | Admin Desktop foundation |
| `859632c` | Draft application service, metadata autosave |
| `2c05d03` | Module/lesson structure editing (add, rename, remove, reorder) |
| `486ff21` | Backend-only native source picker; path-free resource DTOs |
| `27980e2` | Deterministic package staging (ADR 0001) |
| `16999db` | Media inspection warnings, no transcoding |
| `91b9f89` | Deterministic BitTorrent v1 metainfo (ADR 0002) |
| `e8cca5d` | Release preparation job (cancellable, events) |
| `053d792` | Immutable release activation and history (ADR 0003) |
| `224e449` | Publisher CLI parity (`tools/publisher-cli`) |

### Phase 3 — local seeding and metadata-first learner import: done

| Commit | Slice |
|---|---|
| `0c664ea` | `torrent-port` + `torrent-librqbit` adapter (ADR 0004) |
| `7fde20d` | Admin seeding: `SeedingService`, Distribution page, network profile |
| `6a8d42d` / `bea2514` | Learner metadata-first `.torrent` and magnet import |
| `90f656c` | Magnets resolve through local service discovery alone |
| `445da19` | Learner uploads off by default (opt-in) |
| `38b2139` | Per-course learner sharing (`TransferRouter`, sharing engine) |
| `a939d27` | Learner network profile setting |
| `10074ec` | Folder-import failures reported apart from library failures |
| `02874ea` | Fixed peer port and UPnP forwarding, both apps (`phase-3-peer-reachability.md`) |

### Phase 4 — streaming vertical slice: mostly done

| Commit | Slice |
|---|---|
| `9fa9268` | Verified torrent streaming through the gateway (`phase-4-torrent-streaming.md`) |
| `23cdfbe` | Linux in-app playback through an HTTPS loopback gateway (ADR 0005) |
| `2c30cea`, `5655af6` | UI test stability under load |
| `6c9ad4b` | AppImage packaging scripts |

Still open in Phase 4. These also finish the "first milestone" in the roadmap:

1. Playback state machine: loading, buffering, playing, stalled, error.
2. Persisted playback position, restored on reopen and after restart.
3. Separate "buffered" and "verified" range labels in the player.
4. Controlled-swarm automated evidence for the milestone: lesson four plays before completion, a seek into absent data resumes after buffering, and a restart reuses verified bytes.
5. Peer interruption and lookahead or priority behavior, which the spike left uncovered.
6. Playback checks on macOS and Windows. These need those machines; record them as limitations if they are unavailable.

## Active slice: Admin course management (user request 2026-10-16)

The user reported these Publisher gaps:

- there is no course management screen;
- after publishing, the magnet link and `.torrent` file cannot be reached;
- files and folders cannot be added by drag and drop;
- courses and releases cannot be edited, removed, or deleted.

Sub-slice 1 (course management view, release details, and `.torrent` export) is done; see `docs/releases/phase-7-course-management.md`. Continue with sub-slice 2.

### What exists today

- Builder structure editing already adds, renames, removes, and reorders modules, lessons, and resources (`StructureEditor.tsx`, `StructureEdit` commands). Removing a resource detaches it and never touches the source file.
- `features/courses/CourseList.tsx` groups drafts and releases by `course_id`. `ReleaseDetailsPanel.tsx` shows the magnet with Copy (`navigator.clipboard` works on WebKitGTK), the info hash, files (`get_release_details`), and `export_release_torrent`. Each release lives at `<app_data>/releases/<release_id>/`, holding `package/` and `release.torrent` (constants `RELEASE_PACKAGE_DIRECTORY` and `RELEASE_TORRENT_FILE` in `crates/publisher-application/src/releases.rs`). Both are read-only on disk.
- `tauri-plugin-dialog` is already a dependency. `choose_and_attach_resource` opens the native picker in Rust (`commands.rs`).
- Publisher schema is v3 (tables `drafts`, `releases`, `seeding_intents`, `settings`). `releases.status` is one of `published`, `superseded`, or `archived`; archiving is not wired up yet. `seeding_intents.release_id` references `releases`.
- The Courses page (`App.tsx`) shows draft cards and a flat "Releases" list. Drafts and releases share `course_id`.

### User decisions, all made on 2026-10-16

1. **Folder drop builds the structure.** Each top-level dropped folder becomes a module, and each media or PDF file inside it becomes a lesson with that file attached, ordered by name. Loose files dropped onto a lesson attach to that lesson.
2. **Releases: archive, plus optional deletion.**
   - "Archive" stops seeding, removes the seeding intent, and marks the release `archived`. It keeps the bytes.
   - A separate "Delete files" action, available only on archived releases, permanently removes the release directory. It needs a typed confirmation, such as typing the course version.
   - Release deletion is a security-review trigger in AGENTS.md. Never remove the files of a published or superseded release. Never follow symlinks. Validate the release id with `release_directory` and re-check that the target sits under `releases_root`.
3. The peer-reachability slice is finished and committed.

### Planned sub-slices, in order

1. **Course management view and release details.**
   - Group drafts and releases by `course_id` into course cards or rows.
   - Show per-release details: full magnet with a Copy button, info hash, size, files, status, and seeding state.
   - For Copy, use `navigator.clipboard.writeText` with a select-text fallback, or add `tauri-plugin-clipboard-manager` if WebKitGTK refuses.
   - Add an "Export .torrent…" Rust command. It opens a native save dialog and writes the bytes with `fs::write` rather than `fs::copy`, so the exported file isn't read-only. IPC carries only `releaseId`; paths never cross it.
2. **Draft deletion and release archive.**
   - Add `PublisherStorage::delete_draft` and `PublisherDrafts::delete_draft`. Deleting a draft never touches releases.
   - Add `PublisherDrafts::archive_release`. It runs through `SeedingService` (stop, then clear the intent), and `seed_request` and `start` must refuse archived releases.
   - Add confirmation UI, plus tests at storage, application, and command level and in Vitest.
3. **Deleting an archived release's files**, with the guards from decision 2. Remove the files first, then the record, or record a tombstone; choose an approach that recovers cleanly after a crash part-way through, and write an ADR for it. Recovery at startup must also handle a half-deleted directory.
4. **Drag-and-drop of files and folders into the Builder.**
   - Use Tauri v2 window drag-drop events handled in Rust (`on_window_event` with `WindowEvent::DragDrop`). Dropped paths stay in Rust.
   - Emit only a path-free summary to the UI, and keep a short-lived opaque "drop token". The UI then sends `apply_drop { draftId, token, target: course | lessonId }`.
   - Reuse `classify`, `attach_source`, and the bounded media inspection. Bound the recursion depth, file count, and total entries. Skip symlinks and hidden files, and report skipped files.
   - All edits go through the existing single-revision `draftWriter` sequence, so the conflict banner keeps working.

Each sub-slice needs its own release note (for example `docs/releases/phase-7-course-management.md`), acceptance evidence, limitations, a focused commit, and a push. This work pulls part of Phase 7 forward at the user's request.

## Remaining phases (5–10)

Each phase is broken into slices in the same style, ordered by dependency.

### Phase 5 — offline availability and transfer controls

- Download commitments for a lesson, a module, or a whole course, persisted and recovered after restart.
- Accurate availability labels: online-only, downloading, partially available, available offline.
- Pause and resume per course; a cache policy and eviction of learner-downloaded data only, never publisher releases.
- Seeding preferences per course (building on the sharing card) and bandwidth limits where librqbit supports them. Each needs a cited API and an integration test.
- Disk-pressure detection, with graceful refusal or pause.

### Phase 6 — learning workspace

- Progress tracking per lesson and course.
- Tiptap notes with typed anchors: a video timestamp or a PDF page and region.
- PDF.js viewing and an annotation workflow.
- Excalidraw scene persistence with local assets.
- Transcripts and Shiki code highlighting.
- Incremental SQLite FTS5 indexing and offline search.
- Learner data stays strictly outside package directories, and annotations are never overwritten.

### Phase 7 — release and distribution management

Partly pulled forward by the active slice. Remaining:

- version history per course, and supersession/archive UX;
- retry and rollback of failed preparations;
- distribution details and logs, with redaction;
- import and export of publisher configuration;
- suggestions for migrating annotations between course versions.

### Phase 8 — optional headless seeding

- A Linux-first seed server reusing `torrent-port` and `torrent-librqbit`.
- Session restore and a health endpoint.
- Bandwidth policies.
- systemd and Docker deployment.
- Later: authenticated, staged remote transfer of releases and remote management. Remote management is a security-review trigger.

### Phase 9 — optional catalog and publisher identity

- Canonical signed release envelopes and an Ed25519 key lifecycle (key storage is a security trigger).
- A catalog API, discovery, and an explicit publication confirmation.
- Learner torrent and magnet import must keep working without the catalog.

### Phase 10 — hardening and release

- Fuzzing of the manifest, metainfo, and gateway range parsing; a threat-model review; a dependency and license audit.
- A cross-platform media matrix (Linux, macOS, Windows); performance, soak, and fault tests; an accessibility pass.
- Updater signing, reproducible CI packaging, backup and restore, and operator and user documentation.

Open roadmap approval questions remain in `docs/roadmap.md`: platform targets, v1-only BitTorrent, the timing of signing, and license and size policy. Ask the user before a phase depends on one of them.

## Module boundaries

- `course-format`: manifest wire contract and semantic validation.
- `course-publisher`: draft and publishing policy, staging, and media inspection, with no SQLite or Tauri.
- `publisher-storage`: Admin SQLite persistence (schema v3) with compare-and-swap draft saves.
- `publisher-application`: Admin use cases: drafts, sources, staging, torrents, publish jobs, releases, seeding, and settings.
- `torrent-metainfo`: deterministic v1 metainfo writing and inspection.
- `torrent-port`: engine-neutral contract, including `NetworkProfile` and `PeerReachability`.
- `torrent-librqbit`: the only crate that depends on librqbit; read-only seeding storage and explicit session options.
- `learning-application` / `learning-storage`: learner import, library, torrent imports, sharing, and routing.
- `media-gateway`: authenticated loopback HTTP(S) delivery, with no dependency on Tauri or torrents.
- `apps/*/src-tauri`: composition and path-free DTO translation only. Command bodies are plain functions tested against in-memory storage. Errors carry `code`, plus `field` or `currentRevision` where relevant.
- `apps/*/src`: thin React. TanStack Query holds backend data; Zustand holds ephemeral UI state. All Builder writes go through `draftWriter`.

## Review hotspots and constraints

- Course, torrent, and media paths are untrusted. Resolve and validate them in Rust and never accept paths from React.
- The gateway uses IPv4 loopback, a per-process secret, opaque handles, single ranges, 64 KiB chunks, at most eight streams, and timeouts. Linux uses HTTPS with a trusted per-process certificate (ADR 0005).
- `Session::new_with_opts` always binds a UDP socket; set every network option explicitly. Tests use Loopback or `lo`.
- Never serve unverified torrent bytes. Release bytes are immutable. Drafts and releases never share writable state.
- Schema or IPC changes update Rust types, TypeScript contracts, fixtures, compatibility notes, and round-trip tests together.

## Git discipline

- Focused conventional commits pushed to `main`, ending with the `Co-Authored-By` trailer the session's instructions require.
- No force pushes, destructive resets, `git clean`, or commits of `AGENTS.md`.
- A slice is complete when every gate is green, its release note is accurate, its commit is pushed, and `git status --short` shows only ` M AGENTS.md`.
