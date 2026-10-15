# Phase 2: immutable release activation and release history

Status: complete for publishing a verified preparation as an immutable, recorded release and listing release history (see ADR 0003).

## Delivered

- `publisher-storage` schema v2 adds a `releases` table:
  - Columns: release ID, course ID and version, title, source draft and revision, manifest SHA-256, info hash, magnet, piece length and count, file count, total bytes, publish time, status.
  - Constraints: `UNIQUE(course_id, course_version)` and a status `CHECK`.
  - Migration from v1 runs in a transaction and keeps existing drafts. Databases newer than v2 are still rejected.
  - `insert_release` marks the course's prior published release `superseded` in the same transaction. Also new: `version_released`, `release`, and `releases` (newest first).
- `PublisherDrafts::activate_release` follows the ADR order: structural re-check (exact tree, manifest digest, torrent file table and info hash via the new `torrent_metainfo::inspect_torrent`), version-reuse rejection, rename into `release_<id>.partial/`, record commit, read-only payload, final rename. Failures before the commit move the artifacts back to staging.
- `PublisherDrafts::recover_releases` completes recorded partial releases and discards unrecorded ones at startup.
- `PublishJobs::activate` moves a `ReadyToPublish` job to `Published` through the workflow state machine and records the `releaseId` on the job snapshot. `PublishJobs::recover` now discards every session-scoped staging attempt at startup (`discard_all_attempts`).
- Admin backend:
  - Startup runs release recovery, then attempt recovery.
  - New commands: `activate_release(jobId)` (emits the updated `publish-job` snapshot) and `list_releases()`.
  - DTOs expose release metadata with a lowercase `status` and never a path. A reused version maps to the `version_already_released` error code.
- Admin UI:
  - The "Prepare release" panel gains a "Published" step. "Publish release" opens an inline confirmation with focus on Cancel, stating that files and the version number become permanent. Errors such as a reused version are shown, and after publishing the panel shows "Published version X. Its files can no longer change."
  - The Courses page gains a "Releases" history: title, version, date, file count, size, short info hash, and a status badge.

## Acceptance evidence

- Storage tests: supersession on a new version, version reuse rejected without side effects, record round-trip, and a real v1 database file upgraded to v2 with its draft intact.
- Application tests (real files):
  - Activation leaves staging empty and creates exactly one release directory.
  - The payload and torrent are read-only, and writing to the released `course.json` fails.
  - The released package passes the Learner importer.
  - The job becomes `Published` with its release ID; double activation and cancel are refused.
  - Re-preparing the same version is refused at activation while its attempt stays staged; bumping the version publishes and supersedes the first release with a different info hash.
  - An extra staged file blocks activation with nothing moved or recorded.
  - Recovery finishes an interrupted recorded release, drops an unrecorded partial, and ignores unrelated names.
  - A fresh `PublishJobs::recover` clears a completed attempt and its torrent.
- `torrent-metainfo` test: `inspect_torrent` validates structure without package bytes and rejects a mismatched file table.
- Admin command test: preparation then activation through DTOs (`PUBLISHED` stage, matching `releaseId`, `published` status, no paths), and `version_already_released` for a reused version.
- Vitest: confirmation-gated publishing with a reused-version error followed by success, plus release-history rendering and the empty state. Two consecutive full runs passed.
- Manual verification:
  - The real debug binary migrated the existing app-data database created by an earlier build from schema 1 to 2 (adding `releases`) and ran normally.
  - In the browser pane with a fake IPC bridge, the confirmation text and focus were correct. Publishing changed the panel to the published message, and the Courses page listed the new release as Published above the older, now Superseded, release.
- All repository gates pass.

## Current limitations

- Activation re-checks structure but not piece hashes; full re-verification belongs to seeding start.
- Archiving, deleting, or exporting releases is not available yet (Phase 7).
- Restarting the Admin app discards unpublished preparations.
- Cross-volume release locations are not supported.
- Read-only permissions deter accidental edits only; they are not a security boundary.
- The readiness panel does not yet warn that the draft's version is already released; the conflict appears at publish time.
