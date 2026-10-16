# Claude Code handover — Zeron

Handover date: 2026-10-16

Repository: `git@github.com:jehuamanna/zeron.git`, branch `main`

Last pushed commit before this handover: `6c9ad4b build: package the desktop apps as AppImages`

This document replaces the 2026-10-15 handover. A fresh Claude Code session on another machine should be able to continue from it alone.

**Updated 2026-10-16.** Slice A (one Zeron app, `docs/releases/single-zeron-app.md`) and Slice B (Zeron port mapper, `docs/releases/phase-3-port-mapper.md`, ADR 0013) are done. Still open from that plan: a successful mapping on a real router that answers PCP, NAT-PMP, or `UPnP`, and the "Later" items. Otherwise continue with the remaining phases below.

## Mission

Zeron is an offline-first learning platform: one Tauri app, `apps/desktop` (identifier `app.zeron`, ADR 0012), with two areas side by side:

- **Library:** imports, plays, and shares courses (formerly Learner Desktop).
- **Publish:** builds, publishes, and seeds courses (formerly Admin Desktop).

One Network setting covers both. A private engine downloads; a public engine on the one fixed port shares courses and seeds releases.

BitTorrent distributes immutable course packages. It does not own the learning or publishing domain models.

Continue through the dependency-ordered roadmap ([docs/roadmap.md](../roadmap.md)) without waiting between safe, in-scope slices. For each slice: implement it, test it, document acceptance evidence and limitations in a release note under `docs/releases/`, commit, and push to `main`. Pause only for:

- a genuine product decision;
- a missing credential or environment;
- a destructive action outside the approved scope;
- an architectural blocker you can show evidence for.

Read first: [AGENTS.md](../../AGENTS.md) (authoritative rules), [CLAUDE.md](../../CLAUDE.md), [system-overview](../architecture/system-overview.md), [distribution](../architecture/distribution.md), [admin-publisher](../architecture/admin-publisher.md), the ADRs in `docs/decisions/` (0001–0013), and [phase-0 spike results](../research/phase-0-spike-results.md).

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

Build the app with `pnpm --filter @zeron/desktop tauri build --debug --no-bundle` so the frontend is embedded (plain `cargo build` debug binaries load the Vite dev URL). Run a tauri-driver per window you drive, with distinct `--port` / `--native-port`.

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

Packaging: Linux builds are **AppImage only** (user decision 2026-10-16: no `.deb`). Use `pnpm build:appimage` (root) or `pnpm --filter @zeron/desktop build:appimage`. The one Zeron AppImage bundles GStreamer (`bundleMediaFramework: true`). Don't run the slow release AppImage build before pushing unless the user asks.

Real-course fixtures on this machine (user-provided 2026-10-16): `/home/jehu/Downloads/algoe-sixquevideo/` (2.3 GB, 13 H.264/AAC "N Design X.mp4" files; some named `.mp4.mp4`) and `/home/jehu/Downloads/algoexpert-sdfshortcourse/` (1.6 GB, `lesson1.mp4`–`lesson20.mp4` plus `Design Fundamentals.txt`, which lists the lesson titles). Do not use the 30 GB "Back-To-Back-SWE" course. Treat fixtures as read-only input. Publish them only into a scratch directory, keep them on Loopback or Local network, and never commit anything derived from them.

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
| `02874ea`, `68ef538` | Fixed peer port and UPnP forwarding, both apps (`phase-3-peer-reachability.md`, ADR 0004 decision 8) |
| `90e8a96`, (this slice) | Trackers: followed tracker lists (defaults: ngosang and trackerslist.com "best"), additional trackers, only the Internet profile announces (`phase-3-trackers.md`) |

### Phase 4 — streaming vertical slice: mostly done

| Commit | Slice |
|---|---|
| `9fa9268` | Verified torrent streaming through the gateway (`phase-4-torrent-streaming.md`) |
| `23cdfbe` | Linux in-app playback through an HTTPS loopback gateway (ADR 0005) |
| `2c30cea`, `5655af6` | UI test stability under load |
| `6c9ad4b` | AppImage packaging scripts |

`phase-4-streaming-at-startup.md` fixed lessons failing when opened right after startup, and recorded real-window evidence with the user's courses: lesson 4 plays before completion, a seek into absent data resumes, and a restart reuses verified bytes.

Still open in Phase 4. These also finish the "first milestone" in the roadmap:

1. ~~Playback state machine: loading, buffering, playing, stalled, error.~~ Done (`phase-4-player-states.md`).
2. ~~Persisted playback position, restored on reopen and after restart.~~ Done (`phase-4-playback-position.md`, learner schema v5).
3. ~~Separate "buffered" and "verified" range labels in the player.~~ Done (`phase-4-player-states.md`).
4. ~~Controlled-swarm automated evidence for the milestone: lesson four plays before completion, a seek into absent data resumes after buffering, and a restart reuses verified bytes.~~ Done (`phase-4-controlled-swarm-milestone.md`).
5. ~~Peer interruption and lookahead or priority behavior, which the spike left uncovered.~~ Peer loss covered by a two-seeder test; lookahead is librqbit's fixed window (tuning not implemented).
6. Playback checks on macOS and Windows. These need those machines; record them as limitations if they are unavailable.

## Active slice: Admin course management (user request 2026-10-16)

The user reported these Publisher gaps:

- there is no course management screen;
- after publishing, the magnet link and `.torrent` file cannot be reached;
- files and folders cannot be added by drag and drop;
- courses and releases cannot be edited, removed, or deleted.

All four sub-slices are done; see `docs/releases/phase-7-course-management.md`, `phase-7-draft-deletion-and-release-archive.md`, `phase-7-release-file-deletion.md` (ADR 0006), and `phase-7-builder-drag-and-drop.md` (ADR 0007). The native drag gesture still needs one manual check (listed in that note).

### What exists today

- Builder structure editing already adds, renames, removes, and reorders modules, lessons, and resources (`StructureEditor.tsx`, `StructureEdit` commands). Removing a resource detaches it and never touches the source file.
- `features/courses/CourseList.tsx` groups drafts and releases by `course_id`. `ReleaseDetailsPanel.tsx` shows the magnet with Copy (`navigator.clipboard` works on WebKitGTK), the info hash, files (`get_release_details`), and `export_release_torrent`. Each release lives at `<app_data>/releases/<release_id>/`, holding `package/` and `release.torrent` (constants `RELEASE_PACKAGE_DIRECTORY` and `RELEASE_TORRENT_FILE` in `crates/publisher-application/src/releases.rs`). Both are read-only on disk.
- `tauri-plugin-dialog` is already a dependency. `choose_and_attach_resource` opens the native picker in Rust (`commands.rs`).
- Publisher schema is v4 (tables `drafts`, `releases`, `seeding_intents`, `settings`). `releases.status` is one of `published`, `superseded`, or `archived`, and `releases.files_state` is one of `present`, `deleting`, or `deleted` (ADR 0006). `seeding_intents.release_id` references `releases`.
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

## Active plan: one Zeron app, then full port mapping (user decisions 2026-10-16)

Read this section first. Slice A is done: the Mission and the architecture docs now describe one app.

### State at handoff

- **Pushed and green:**
  - peer reachability and UPnP forwarding, now on by default (`07f1164`, ADR 0004 decision 8);
  - followed tracker lists plus additional trackers (`bc5ec94`);
  - uTP alongside TCP (`443ca51`, `docs/releases/phase-3-utp.md`).
- **Real internet test (2026-10-16):**
  - **Setup:** the Admin seeded the systems-design fixture under Internet on fixed port 51413 with 70 public trackers. A Learner on a different network never connected.
  - **Admin's router:** answers no UPnP (SSDP), NAT-PMP, or PCP. The address is public IPv4, not carrier-grade NAT, but trackers see a different egress address in the same /16 than `api.ipify.org` does, so the provider may NAT through a pool.
  - **What the trackers show:** they list the seeder on 51413, and a TCP probe to it from outside is dropped. The other machine's Learner ports were dropped too. Neither side accepted connections, so no client could connect.
  - **Ways out:** a manual TCP 51413 forward on the publisher's router, a learner network that accepts connections (UPnP, NAT-PMP, or PCP). (The Phase 8 seed server was removed from the roadmap.)
  - **Scratch-only state:** the scratch Publisher data (`publisher-run`) and the public magnet live only in that session's scratchpad. The user asked that the magnet be shared only with the Learner and destroyed afterwards. Stop that seeding and archive or delete its release when the test ends.

### Slice A: merge Publisher and Learner into one app `apps/desktop` — done

Delivered from `fbed259` through the commit that removed `apps/admin-desktop`; see `docs/releases/single-zeron-app.md`, ADR 0011 (data import), and ADR 0012 (one app). The decisions and facts below are kept for reference.

User decisions:

1. **Identity and data:**
   - One app, product name "Zeron", identifier `app.zeron`.
   - On first start, import old data once from `app.zeron.learner` and `app.zeron.publisher`.
2. **Navigation:** top-level Library (learning) and Publish (courses, builder, distribution) side by side, always visible.
3. **Engines:**
   - One network setting for the whole app: profile, fixed port, UPnP, rate limits.
   - Two engines:
     - a **private** engine for downloads, with uploads off;
     - a **public** engine, uploads on, seeding published releases and the courses the learner shares, on the one fixed port.
4. **Repository:** replace `apps/admin-desktop` and `apps/learner-desktop` with `apps/desktop`. Package `@zeron/desktop`, one AppImage (`bundleMediaFramework: true` stays).

Facts gathered for the merge:

- **Command collisions:** `set_network_profile` and `set_peer_reachability` exist in both apps. The Admin has 26 other commands and the Learner 33. Keep one network command set; the Admin's Distribution page uses the Learner's network commands. Rate limits: the Learner has `set_rate_limits`, and the Admin applies limits to its engine in `lib.rs`; unify them.
- **Settings ownership:** network settings live in `library.sqlite3` (`CourseLibrary`). Migrate the Publisher's `network_profile`, `peer_port`, and `port_forwarding` only if the Learner has none. Tracker settings (`trackers`, `tracker_lists`, `tracker_list:*`) stay in `publisher.sqlite3`.
- **Stored paths:** the Learner DB stores absolute torrent `content_root` paths (`learning-storage`, `TorrentCourseSource`, encoded with `decode_path`), for example `…/app.zeron.learner/courses/<id>`. Rewrite these with the prefix swap from the old to the new app-data directory. Local-folder imports and draft sources point outside app data and are unaffected. Publisher releases are derived from `releases_root` and need no rewrite.
- **What moves, using `rename` on the same filesystem:**
  - Learner: `library.sqlite3` and `courses/`.
  - Publisher: `publisher.sqlite3`, `releases/` (read-only directories; keep their permissions), and `staging/`.
  - `engine*` state directories hold no persistent state; do not move them.
  - The migration needs a marker file and must be crash-safe and idempotent: copy, verify, switch, then remove. Write an ADR for it (path handling is a security-review trigger).
- **Same course on both sides:** a release the user both seeds and has imported has the same info hash in the public engine. `SeedingService` and `TransferRouter` must not fight over it: refuse or redirect importing your own release, or let seeding take precedence. Decide, test, and document.
- **Frontend:**
  - The Learner app is the base (more features). Move the Admin's `features/{builder,courses,distribution,publish}`, `services/publisher.ts`, `stores`, and their tests under a Publish area. Add top-level navigation.
  - The Distribution page's Network and reachability forms become the shared Network settings, so remove the duplicate Learner/Admin forms.
  - Merge the CSPs. The Learner's `media-src` already allows the HTTPS gateway; nothing new from the Admin needs to be added.
- **Rust:**
  - Move the Admin's `commands.rs`, `drops.rs`, and `tracker_lists.rs` into the merged crate as a `publish` module.
  - Merge `AppState` (library, gateway, torrents, plus drafts, publish jobs, drops, distribution).
  - One composition root starts the gateway (HTTPS on Linux), the two engines, `SeedingService` on the public engine, the router, imports, sharing, the startup tracker-list refresh, and seeding restore.
  - Shutdown stops both engines and the gateway.
- **Docs:**
  - A new ADR, "Single desktop app", superseding the separate-packaging decision.
  - Update `system-overview.md`, `distribution.md`, `admin-publisher.md`, `roadmap.md`, this handover's Mission, the root `package.json` scripts, and the AppImage memory note.
  - `AGENTS.md` is user-owned: suggest wording changes to the user, never commit them.
- **Acceptance:**
  - All gates pass.
  - Vitest for navigation, plus the moved suites.
  - Rust tests for migration: fresh install, both old directories, one old directory, a crash part-way through, and a second run that does nothing.
  - **Real-window check:** start the merged app with copies of both old data directories. Library, drafts, and releases appear; a published release seeds from the public engine; a magnet import streams on the private engine; a shared course seeds on the public engine; one port is shown in Network.

Suggested order (keep every commit green): scaffold `apps/desktop` from the Learner (git mv, rename identity) → move the Publisher backend in, behind the merged state → unify the network settings and the two engines → move the Publisher frontend and navigation → data migration with its ADR → delete the old apps and update scripts and docs.

### Slice B: Zeron port mapper (UPnP for TCP and UDP, NAT-PMP, PCP), as in Deluge — done

Delivered; see `docs/releases/phase-3-port-mapper.md` and ADR 0013. The user approved `netdev` on 2026-10-16, and it is used for gateway discovery on every platform. The plan below is kept for reference.

- **Why:** librqbit's UPnP maps TCP only (`librqbit-upnp` 9.0.1 `forward_port` hard-codes `TCP`), so uTP cannot be reached from outside. It has no NAT-PMP or PCP, and it reports nothing back. Deluge (libtorrent) has UPnP and NAT-PMP on by default.
- **Plan:**
  - A new crate, `port-mapping`, with no torrent dependency.
  - **UPnP IGD:** SSDP discovery, the description document, then SOAP `AddPortMapping` and `DeletePortMapping` over `reqwest`, which is already compiled in.
  - **NAT-PMP** (RFC 6886) and **PCP** (RFC 6887): MAP requests for TCP and UDP to the default gateway.
  - **Gateway discovery:** `/proc/net/route` on Linux. Other platforms need a small crate such as `netdev`; confirm with the user before adding it.
  - Lease renewal at half-life, removal on shutdown, and a status per protocol and transport (mapped, external port, error).
  - Turn off librqbit's `enable_upnp_port_forwarding` and drive the mapper from the composition root for the public and private engine ports.
  - Show the status in Network: "Router: port 51413 open via NAT-PMP (TCP and UDP)", or "No router answered UPnP, NAT-PMP, or PCP. Forward TCP and UDP 51413 by hand."
- **Tests:** fake IGD, NAT-PMP, and PCP servers on loopback (unit and integration), plus lease-renewal and shutdown-removal tests.
- **Real check:** this user's router answers none of the three protocols, so verify against a router that does, and record which protocols were tested.

### Later (record only)

- **Hole punching (BEP 55, `ut_holepunch`) and protocol encryption (MSE/PE):** Deluge has both; librqbit 9.0.1 has neither. Each is a substantial engine change; propose it as an upstream contribution or a fork, and ask the user first.
- **Paid or private courses:** parked by the user; needs encryption, key delivery, accounts, and payments. See the conversation summary in `phase-3-trackers.md` limitations.

## Remaining phases (7, 9, 10)

Each phase is broken into slices in the same style, ordered by dependency.

### Phase 5 — offline availability and transfer controls

Done so far: engine file selection (`phase-5-file-selection.md`, ADR 0004 decision 9) and lesson/module/course download commitments with availability labels (`phase-5-download-commitments.md`, learner schema v6), per-course pause/resume (`phase-5-pause-and-resume.md`, learner schema v7, ADR 0004 decision 10), speed limits in both apps (`phase-5-speed-limits.md`, ADR 0004 decision 11), streamed-lesson removal with a cache cap (`phase-5-cache-eviction.md`, ADR 0008), and disk-pressure handling (`phase-5-disk-pressure.md`). **Phase 5 is complete**; see `phase-5-complete.md`. The list below is kept for reference.

- Download commitments for a lesson, a module, or a whole course, persisted and recovered after restart.
- Accurate availability labels: online-only, downloading, partially available, available offline.
- Pause and resume per course; a cache policy and eviction of learner-downloaded data only, never publisher releases.
- Seeding preferences per course (building on the sharing card) and bandwidth limits where librqbit supports them. Each needs a cited API and an integration test.
- Disk-pressure detection, with graceful refusal or pause.

### Phase 6 — learning workspace

Done so far: lesson and course progress (`phase-6-progress.md`, new `learning-domain` crate, learner schema v8) Tiptap notes with typed anchors (`phase-6-notes.md`, ADR 0009, learner schema v9), PDF viewing with page and highlight notes (`phase-6-pdf-viewer.md`), and Excalidraw lesson drawings with write-once images and a Learner navigation guard (`phase-6-drawings.md`, ADR 0010, learner schema v10), and synced lesson transcripts parsed from WebVTT or SubRip (`phase-6-transcripts.md`), a Markdown and text lesson reader with Shiki highlighting (`phase-6-reader-and-code-highlighting.md`), and offline FTS5 search over courses, lessons, notes, transcripts, and lesson text (`phase-6-offline-search.md`, learner schema v11). **Phase 6 is complete**; its real-window search check is listed in that note.

- Progress tracking per lesson and course.
- Tiptap notes with typed anchors: a video timestamp or a PDF page and region.
- PDF.js viewing and an annotation workflow.
- Excalidraw scene persistence with local assets.
- Transcripts and Shiki code highlighting.
- Incremental SQLite FTS5 indexing and offline search.
- Learner data stays strictly outside package directories, and annotations are never overwritten.

### Phase 7 — release and distribution management

All items are done:

- ~~version history per course, and supersession/archive UX~~ done (`phase-7-version-history.md`: make current, new draft from a release);
- ~~retry and rollback of failed preparations~~ done (`phase-7-preparation-retry-and-rollback.md`);
- ~~distribution details and logs, with redaction~~ done (`phase-7-distribution-activity.md`, publisher schema v5);
- ~~import and export of publisher configuration~~ done (`phase-7-publisher-configuration.md`);
- ~~suggestions for migrating annotations between course versions~~ done (`phase-7-annotation-migration.md`, learner schema v12).

**Phase 7 is complete.**

### Phase 8 — removed

The user removed the headless seed server on 2026-10-16 (an earlier attempt, `f18d123`, was reverted in `628352b`). Do not reintroduce it.

### Phase 9 — optional catalog and publisher identity

The user wants signing, working on Windows and macOS without limitation (2026-10-16; ADR 0014). Done so far: the signature formats and key handling (`phase-9-signing-foundation.md`, crates `course-signing` and `publisher-identity`) and a three-platform CI matrix (`.github/workflows/ci.yml`). Next: sign releases when publishing, verify and pin in the Library, then catalogs.

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
- `publisher-storage`: Admin SQLite persistence (schema v5: drafts, releases, seeding intents, settings, distribution events) with compare-and-swap draft saves.
- `publisher-application`: Admin use cases: drafts, sources, staging, torrents, publish jobs, releases, seeding, and settings.
- `torrent-metainfo`: deterministic v1 metainfo writing and inspection.
- `torrent-port`: engine-neutral contract, including `NetworkProfile` and `PeerReachability`.
- `torrent-librqbit`: the only crate that depends on librqbit; read-only seeding storage and explicit session options.
- `learning-domain`: pure learner rules (progress now, annotations next).
- `learning-application` / `learning-storage`: learner import, library, torrent imports, sharing, and routing.
- `media-gateway`: authenticated loopback HTTP(S) delivery, with no dependency on Tauri or torrents.
- `course-signing`: pure Ed25519 keys, canonical statements, release signatures, rotation chains, revocations, and catalogs (ADR 0014).
- `publisher-identity`: the publisher's signing key in the OS credential store, rotation, and passphrase backups.
- `port-mapping`: PCP, NAT-PMP, and `UPnP` IGD mapping for TCP and UDP, with renewal, removal, and status (ADR 0013).
- `data-import`: one-time, crash-safe import of the former Learner and Publisher data into `app.zeron` (ADR 0011).
- `apps/desktop/src-tauri`: composition and path-free DTO translation only; Publish commands live in its `publish` module. Command bodies are plain functions tested against in-memory storage. Errors carry `code`, plus `field` or `currentRevision` where relevant.
- `apps/desktop/src`: thin React; Library at `app/Library.tsx`, Publish under `publish/`, the shell in `app/App.tsx`. TanStack Query holds backend data; Zustand holds ephemeral UI state. All Builder writes go through `draftWriter`.

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
