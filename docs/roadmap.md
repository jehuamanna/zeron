# Dependency-ordered roadmap

## Phase 0 — architecture and feasibility

Approve schemas, trust boundaries, crate graph, and state machines. Build disposable experiments for librqbit metadata/file selection/verified reads/priorities/resume/seeding and Tauri WebView loopback Range playback. Decide the exact torrent creation library after a golden-fixture comparison. Exit only with a capability matrix and approved gap strategy.

## Phase 1 — local course vertical foundation

Create the workspace only for `course-format`, storage, application shell, and learner app. Implement schema + fixtures, safe local-folder import, SQLite migrations, course tree, complete-file basic video playback, readiness, and recovery. This validates contracts without coupling the domain to torrents.

## Phase 2 — admin builder and reproducible publishing

Add the Publisher (then a separate Admin Desktop, now the Publish area of the one app; see ADR 0012) and `course-publisher`: draft CRUD/autosave, hierarchy/reorder, validation, media probing, deterministic package/manifest generation, v1 torrent generation and verification, immutable release activation, and CLI parity. No cloud dependency.

## Phase 3 — local seed to metadata-first learner import

Integrate the approved torrent adapter once for both apps. Admin starts/restores local seeding. Learner imports `.torrent` and magnet, resolves metadata, fetches only `course.json`, validates it against torrent inventory, and renders real navigation. Add dashboard-local metrics and explicit upload controls.

## Phase 4 — first production-shaped vertical slice

Implement the authenticated loopback Range gateway, playback sessions, verified-range waiting, bounded buffer control, priority/cancellation policy, seek, lesson switching, and restart reuse. Demonstrate lesson four playback before completion and seek into an unavailable range without downloading the rest of the course. Broad feature work is gated on this result.

## Phase 5 — offline availability and transfer controls

Add lesson/module/course download commitments, accurate availability labels, pause/resume, cache policy, recovery, seeding preferences, bandwidth controls where supported, and disk-pressure handling.

## Phase 6 — learning workspace

Add progress, Tiptap notes, typed video/PDF anchors, PDF.js annotation workflow, Excalidraw scene persistence with local assets, transcripts, Shiki, incremental FTS5 indexing, and offline search. Preserve strict separation from packages.

## Phase 7 — release and distribution management

Add version history, supersession/archive, retry/rollback UX, distribution details/logs, publisher configuration import/export, and annotation migration suggestions.

## Phase 8 — optional headless seeding

Create the Linux-first seed server, session restore, health, bandwidth policies, systemd/Docker deployment, then authenticated staged remote transfer and management.

## Phase 9 — optional catalog and publisher identity

Specify canonical signed release envelopes, Ed25519 key lifecycle, catalog API, discovery, and explicit publication confirmation. Learner torrent import remains independent.

## Phase 10 — hardening and release

Complete fuzzing, threat-model review, dependency/license audit, cross-platform media matrix, performance/soak/fault tests, accessibility, updater signing, reproducible CI packaging, backup/restore, and operator/user documentation.

## First milestone

The first meaningful milestone spans Phases 0–4: Admin publishes and seeds a four-lesson course; Learner imports its magnet, retrieves the manifest first, selects lesson four, starts before completion, seeks into absent data, resumes after buffering, and restarts without losing verified bytes or a timestamp note. It is complete only with controlled-swarm automated evidence and all gateway security checks.

## Approval questions

1. Are Windows, macOS, and Linux all launch targets for the first streaming milestone, or may Linux be the spike platform with cross-platform gating before beta?
2. Is BitTorrent v1-only acceptable for the first release despite SHA-1 being part of its content identity, with SHA-256 retained for resource validation?
3. May Phase 0 adopt a narrow maintained metainfo writer if librqbit has no supported creation API?
4. Should publisher signing wait for Phase 9, or is authenticity required in the first private beta?
5. What license and maximum package/manifest/file-count policies should the default validator enforce?

