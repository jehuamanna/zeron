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

Status: complete (2026-10-16); see the `phase-7-*` notes in `docs/releases/`.

## Phase 8 — removed

The user removed the headless seed server from the roadmap on 2026-10-16. Publishers seed from the desktop app; the later phases keep their numbers.

## Phase 9 — optional catalog and publisher identity

Specify canonical signed release envelopes, Ed25519 key lifecycle, catalog API, discovery, and explicit publication confirmation. Learner torrent import remains independent.

Status: complete (2026-10-16); see ADR 0014 and `docs/releases/phase-9-complete.md`.

## Phase 10 — hardening and release

Complete fuzzing, threat-model review, dependency/license audit, cross-platform media matrix, performance/soak/fault tests, accessibility, updater signing, reproducible CI packaging, backup/restore, and operator/user documentation.

Status: complete (2026-10-16), apart from the steps that need the maintainer's credentials or hardware; see `docs/releases/phase-10-complete.md`.

## Phase 11 — paid courses and learner licenses

Let a publisher sell a course without Zeron accounts, following [ADR 0017](decisions/0017-paid-courses-and-licenses.md):

- paid lessons are encrypted in the package;
- a license issuer signs licenses bound to the learner's key after payment;
- learners get their purchases back on a new machine by signing in with Google or GitHub;
- publishers can prove their GitHub identity.

Free courses and magnet/`.torrent` import do not change and never contact an issuer.

Slices, in dependency order:

1. **Spike.** Disposable experiments under `spikes/`:
   - the system-browser OAuth loopback flow on Windows, macOS, and Linux;
   - seek latency through a chunked-decrypting gateway reader;
   - choosing the HPKE crate.

   Exit with results in `docs/research/`.

   Done 2026-10-16: see [paid-courses-spike-results.md](research/paid-courses-spike-results.md). The choices are 64 KiB sealed chunks and the `hpke` crate 0.14.1; decryption adds no measurable seek cost, and the issuer-as-relying-party sign-in works in a real browser. Real providers, Windows/macOS, and AppImage browser launch move to slice 7.
2. **Encrypted packages.**
   - Manifest schema bump with `access` and per-resource `encryption`.
   - Chunked XChaCha20-Poly1305 in `course-access`.
   - Publish marks lessons as paid or free preview, and encrypts paid resources at packaging.
   - The gateway, PDF/transcript readers, and indexer decrypt with a locally supplied key.
   - Golden fixtures and range or seek tests over encrypted media.

   Done 2026-10-16: see [phase-11-encrypted-packages.md](releases/phase-11-encrypted-packages.md). Until licenses exist, the Library uses the publishing computer's own content keys; everyone else sees paid lessons as locked.

3. **Learner identity.**
   - A learner key in the OS credential store on all three platforms.
   - An optional passphrase backup and restore, matching the ADR 0014 format.

   Done 2026-10-16: see [phase-11-learner-identity.md](releases/phase-11-learner-identity.md).

4. **Offline licenses.**
   - License, issuer-delegation, and revocation verification against the pinned publisher chain.
   - HPKE unwrapping.
   - Licenses stored as learner data and imported from a file.
   - A paid course unlocks with no network.

   Done 2026-10-17: see [phase-11-offline-licenses.md](releases/phase-11-offline-licenses.md). Publishers can also license a buyer by hand, with no issuer.

5. **Reference license issuer** (`services/license-issuer`):
   - release registration with sealed content keys;
   - license issuance for requests signed by a learner key;
   - later-version keys;
   - controlled local tests with no public network.

   Done 2026-10-17: see [phase-11-license-issuer.md](releases/phase-11-license-issuer.md) and the [operator guide](operations/license-issuer.md). Entitlements are granted by command until slice 6.

   **Slices 6–8 are pending** (user decision 2026-10-17). Taking payments and running license servers are on hold until the payment provider and who hosts issuers are decided (ADR 0017 open questions 1 and 2). The issuer from slice 5 is built and tested but not deployed, and the app does not talk to it. Until then, paid courses are sold by licensing each buyer by hand (slice 4).

6. **Payment** (pending). One provider's checkout and webhook, idempotent purchase recording, and refunds that stop re-issue.
7. **Sign-in with Google or GitHub** (pending; the issuer is the sign-in relying party).
   - Issuer-side OIDC/OAuth.
   - The desktop loopback and PKCE flow through the system browser.
   - Re-issue on a new machine.
   - Device list and device limit in the Library.
8. **Publish integration** (pending; it connects Publish to an issuer).
   - Paid toggle and preview lessons. Already delivered in slice 2.
   - Issuer configuration and delegation signing.
   - Release registration, with recoverable retries when the issuer is unreachable.
9. **Creator verification.** Signing and posting the GitHub gist proof, the catalog field, and the Library's online check with the "verified via GitHub" label.

Exit:

- A publisher sells a three-lesson course with one free preview.
- A learner previews it, buys it, streams and seeks a paid lesson, and restarts offline with the course still unlocked.
- The learner signs in on a second machine and gets the course back.
- A refunded purchase stops being re-issued.
- The threat model covers the issuer, licenses, and the decrypting gateway.
- Every slice has its release note.

The exit criteria need slices 6–8, which are pending. Slice 9 does not depend on them.

## First milestone

The first meaningful milestone spans Phases 0–4: Admin publishes and seeds a four-lesson course; Learner imports its magnet, retrieves the manifest first, selects lesson four, starts before completion, seeks into absent data, resumes after buffering, and restarts without losing verified bytes or a timestamp note. It is complete only with controlled-swarm automated evidence and all gateway security checks.

## Approval questions

1. ~~Are Windows, macOS, and Linux all launch targets?~~ Answered 2026-10-16: yes. Signing must work on Windows and macOS without limitation, and all three platforms build and test in CI.
2. Is BitTorrent v1-only acceptable for the first release despite SHA-1 being part of its content identity, with SHA-256 retained for resource validation?
3. ~~May Phase 0 adopt a narrow maintained metainfo writer?~~ Settled by ADR 0002: Zeron writes v1 metainfo itself (`torrent-metainfo`).
4. ~~Should publisher signing wait for Phase 9?~~ Answered 2026-10-16: the user wants signing now (ADR 0014).
5. What license and maximum package/manifest/file-count policies should the default validator enforce?
6. Phase 11: who hosts license issuers, which payment provider comes first, the default device limit, and whether purchases include future versions (ADR 0017 open questions). Payments and license servers are on hold since 2026-10-17, so Phase 11 slices 6–8 are pending.

