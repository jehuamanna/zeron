# Phase 1 completion note: storage foundation

Date: 2026-10-15

## Delivered scope

- Added `learning-storage` using exactly pinned `rusqlite` with bundled SQLite.
- Added transactional schema version 1 migration with strict course/resource tables and cascading foreign keys.
- Added startup negotiation/checks for foreign keys, WAL-compatible journal mode, and compiled/working FTS5.
- Added transactional imported-course upsert, resource replacement, and search-index refresh.
- Added deterministic course listing, summary lookup, validated manifest snapshot loading, and bound FTS queries.
- Kept `rusqlite` connections, rows, and database models private to the storage crate.
- Documented the synchronous API as a dedicated database-thread boundary for later desktop composition.

## Acceptance evidence

Formatting, strict Clippy, and all workspace tests pass. Five storage tests cover migration/capability startup, transactional save/load/replacement and FTS refresh, future-schema refusal, SQLite integer overflow, and corrupt snapshot rejection. The workspace now has sixteen passing tests.

## Manual verification

Open a file-backed database with `Storage::open`, import a valid manifest through `save_imported_course`, restart the process, and confirm `course`, `course_manifest`, `list_courses`, and `search_courses` return the durable snapshot. Inspect `PRAGMA user_version`, `foreign_keys`, and `journal_mode` with SQLite tooling if available.

## Known limitations

- The composition-root database worker and async request queue are not implemented yet; callers must not run storage methods on Tokio worker threads.
- Schema version 1 stores library metadata only. Learner progress, annotations, torrent sessions, and drafts will receive separate migrations.
- Search currently covers course title, description, publisher, and tags. Lesson/resource/transcript/note indexing belongs to Phase 6.
- Backup/restore and migration fault-injection arrive in hardening phases.

## Next dependency-ordered slice

Wire local verification and persistence into an application service with explicit startup readiness and recovery outcomes, then expose it through a thin CLI composition root. This proves the full local import flow before Tauri/UI scaffolding.
