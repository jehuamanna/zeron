# Phase 7 (pulled forward): deleting an archived release's files

Status: complete. This is sub-slice 3 of the Admin course management work. Publishers can permanently remove the package and torrent of an archived release after typing its version. The record stays, so the version is never reused. See [ADR 0006](../decisions/0006-release-file-deletion.md).

## Delivered

- **`publisher-storage` schema v4:**
  - `releases.files_state` is one of `present`, `deleting`, or `deleted`. Existing rows migrate as `present`.
  - New `ReleaseFiles` enum on `ReleaseRecord`.
  - `begin_release_file_deletion` (archived releases only), `finish_release_file_deletion`, and `releases_deleting_files`.
- **`publisher-application`:**
  - `PublisherDrafts::delete_release_files(releases_root, release_id, confirmation)` follows ADR 0006: validate, record `deleting`, re-check containment, move aside, remove without following links, then record `deleted`. It is idempotent once deleted.
  - `recover_release_deletions` finishes interrupted deletions. The new `DraftServiceError::ReleaseNotArchived` refuses releases still in use.
- **Admin:**
  - `delete_release_files { releaseId, confirmation } → Release`, with error codes `release_not_archived`, `invalid_input` (`field: "confirmation"`), and `release_delete_failed`.
  - `Release` gains `files`.
  - Startup runs deletion recovery after activation recovery. Failures are logged without paths and do not block startup.
- **UI:**
  - Archived releases offer "Delete files…". The confirmation states the size and the consequences, and its button stays disabled until the exact version is typed.
  - A deleted release shows "Files deleted" in its row, keeps its magnet, and hides export and the file list. A release stuck in `deleting` offers a retry, and its torrent is never read.

## IPC and storage compatibility

- **IPC:** one new command and a new `files` field on `Release`, which is additive.
- **Storage:** the migration from v3 to v4 is `ALTER TABLE … ADD COLUMN … DEFAULT 'present'`. Older Admin builds refuse a v4 database with `FutureSchema`.

## Acceptance evidence

- **Storage tests:**
  - migrating v3 to v4 keeps rows as `present`;
  - the deletion state machine works: only archived releases can begin, begin is idempotent, finish moves to `deleted`, and the version stays released.
- **Application tests:**
  - **Refusals:** published and superseded releases are refused; path-like, unknown, and malformed ids are refused; wrong confirmations are refused.
  - **Deletion:** succeeds with the version typed with surrounding spaces. Only that directory is removed, the call is idempotent, and export and seeding then fail. Re-publishing the same version is still refused, and the other release stays `present`.
  - **Symbolic links** (Unix): a directory link and a file link inside the package, and a release directory replaced by a link to an outside folder, all leave the outside file intact.
  - **Crash recovery** (Unix): an interrupted rename, forced by making the releases root read-only, leaves `deleting` with the directory intact. A half-removed `.deleting` directory is ignored by activation recovery and finished by `recover_release_deletions`, which is idempotent.
- **Command test:** a wrong confirmation maps to `invalid_input` and `confirmation`. A successful deletion returns `files: "deleted"` with no paths, and the directory is gone.
- **Vitest:** the button stays disabled until the exact version is typed; an error is shown and can be retried; the deleted state hides export and files; the retry path for `deleting` never reads the torrent.
- **Real window** (WebKitGTK through `tauri-driver`):
  - Opening the scratch data directory created by the previous slice migrated it from v3 to v4.
  - **Typed deletion:** a wrong version kept the button disabled, and typing `1.1.0` enabled it. Deletion removed the directory, and the row read "Files deleted · Archived", including after a restart.
  - **Startup recovery:** the other release was set to `archived`/`deleting` in SQLite and its directory renamed to `.deleting`. On launch, the directory was removed and the row became `deleted`.
- All repository gates pass.

## Current limitations

- Deletion of read-only files on Windows has not been tested.
- There is no undo, and the record cannot be purged. Both are deliberate.
- Disk space is reported from the release record, not measured on disk.
