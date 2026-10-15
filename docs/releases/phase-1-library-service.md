# Phase 1 completion note: local library service

Date: 2026-10-15

## Delivered scope

- Added `CourseLibrary` as the application-owned boundary composing package verification and durable storage.
- Ensured failed or cancelled verification cannot create a visible library entry.
- Added path-free import receipts, deterministic listing, validated manifest recovery, and course metadata search.
- Added a restart integration test using a file-backed SQLite database.
- Added `course-cli` as a thin composition root supporting `import`, `list`, and `search` without duplicating business logic.

## Acceptance evidence

Formatting, strict Clippy, and all workspace tests pass. Two new end-to-end service tests prove that a verified import survives process-level database reopen and that tampered content never becomes visible. The workspace now has eighteen passing tests.

## Manual verification

Run the three `course-cli` commands documented in the repository README against a valid local package. Restart between import and list, then corrupt one resource and verify a new import fails without replacing the prior durable snapshot.

## Known limitations

- The service remains synchronous. Tauri will own a bounded database/import worker and publish job snapshots rather than invoking it on an async runtime worker.
- The CLI intentionally has a narrow manual argument parser; it is a validation/composition tool, not the final publisher CLI.
- Storage records the verified manifest and resource metadata but not the local course root. A later backend-only location repository must preserve that path without sending it over IPC.
- Complete-file media playback and a learner desktop UI remain outstanding in Phase 1.

## Next dependency-ordered slice

Scaffold the Learner Desktop with a thin Tauri 2 composition root and strict React/TypeScript client, then expose readiness, list, local-folder import, and manifest navigation through versioned IPC DTOs. Media bytes remain outside IPC; the first player uses a fully verified local file under a narrowly registered asset protocol or loopback resource handle.
