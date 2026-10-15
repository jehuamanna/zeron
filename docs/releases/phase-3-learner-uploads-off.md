# Phase 3: learners upload nothing by default

Status: complete for the default. The Learner's torrent engine never serves course data to other peers. The Admin's engine still seeds. The user decided on 2026-10-15 that learner sharing is off by default and opt-in; how a learner opts in is the next slice.

## Delivered

- **Workspace:** librqbit is built with its `disable-upload` feature. librqbit applies this session-wide: every torrent in a session copies the session's setting (`session.rs`), and there is no per-torrent switch.
- **`torrent-librqbit`:** `EngineConfig::allow_uploads` is required and has no default.
  - When it is off, librqbit sends no bitfield or have messages, rejects every piece request from a peer, and stops dialing peers once its selected files are complete.
  - `seed` is refused with `UnsupportedCapability`, because a seeding engine must upload.
- **Learner app:** starts its engine with `allow_uploads: false`.
- **Admin app:** starts its engine with `allow_uploads: true`.
- **Tests:** seeders opt in explicitly, and learner engines in the import tests run with uploads off, as the app does.

## Acceptance evidence

- **New loopback test `engines_with_uploads_disabled_serve_no_pieces`:**
  - Setup: a private learner (uploads off) and a sharing learner (uploads on) both download `course.json` from a seeder. The seeder then shuts down.
  - Against the private learner, a third engine verifies 0 bytes in 5 s, and the private learner reports 0 bytes uploaded.
  - The same request against the sharing learner completes.
  - The private learner's `seed` request is refused.
  - **Mutation check:** with uploads switched on for the "private" learner, the test fails: the third engine received all 2,043 bytes. The assertion is not vacuous.
- **Existing coverage with uploads off:** all magnet and `.torrent` import tests pass with the learner's uploads off, including restore after restart and the opt-in discovery-only import (about 2.2 s).
- **Real windows:** with WebDriver, the Admin seeding and the rebuilt Learner (uploads off) imported the release over a magnet. Metadata arrived at +0.3 s, `course.json` at +2.4 s, and the course was added at +2.9 s. It persisted across a Learner restart.
- All repository gates pass.

## Current limitations

- Opt-in arrived with `phase-3-learner-course-sharing.md`: per-course sharing through a second, upload-enabled engine session.
- Learner upload metrics arrived with per-course sharing.
- The setting is fixed when the engine starts.
