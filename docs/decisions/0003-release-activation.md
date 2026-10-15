# ADR 0003: Release activation, immutability, and session-scoped preparations

Status: accepted (2026-10-15)

## Context

A prepared release is a verified staging attempt plus its torrent. Publishing must turn it into an immutable release that cannot collide with another version, survives crashes at any point, and is never partially visible. The specification requires `(courseId, version)` to be unique and forbids reusing a version for different bytes. Preparation jobs live only in memory.

## Decision

1. **Layout.** A release lives in `<app data>/releases/<release_id>/` and holds `package/` (the exact staged tree, including `course.json`) and `release.torrent`. Release IDs are opaque `release_<32 hex>` values. Paths stay inside the Admin backend.
2. **Activation order.**
   1. Re-check the attempt structurally: exact tree, manifest SHA-256, the torrent's strict decode and file table, and the info hash against the prepared receipt. Full piece re-verification is deferred to seeding start.
   2. Reject a version that is already released.
   3. Create `<release_id>.partial/` and move the package and torrent into it by rename. Staging and releases share the app-data filesystem.
   4. Commit the release row.
   5. Make all payload files read-only.
   6. Rename the directory to `<release_id>/`.

   Any failure before the commit moves the artifacts back to staging.
3. **Recovery at startup.** A `release_<hex>.partial/` directory with a committed row is finished (step 2.v–2.vi). One without a row is discarded, since it holds only derived copies. Other names are ignored.
4. **Supersession.** Committing a release marks the course's previously published release `superseded` in the same transaction. Superseded releases keep their files. Archiving and deletion are separate, explicit operations for a later phase.
5. **Preparations are session-scoped.** Because preparation jobs live in memory, nothing can activate an attempt after a restart. Startup therefore discards every staging attempt and torrent (completed or partial) whose name matches the attempt patterns. Prepared artifacts are deterministic and can be regenerated.
6. **Explicit consent.** The Admin UI publishes only after an inline confirmation stating that the files and version become permanent.

## Consequences

- A crash at any step leaves either the staged attempt (discarded at the next startup), an unrecorded partial release (discarded), or a recorded release that startup completes. No half-visible release appears in the history.
- Read-only permissions guard against accidental edits but are not a security boundary against the local user.
- Cross-volume activation (copy, verify, activate) is not implemented; the app-data layout keeps staging and releases on one filesystem.
- Restarting the Admin app discards unpublished preparations, which must be prepared again.
