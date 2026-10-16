# Phase 10: backup and restore

Status: complete. Settings → Backup saves one file holding the Library's and Publish's databases, and restores such a file the next time Zeron starts. The current data is kept beside it.

## Delivered

- **`data-backup`** (new crate):
  - **File format:** a magic line `ZERON-BACKUP`, a length-prefixed JSON header (`format`, `version`, `createdAt`, and per database its name, length, SHA-256, and schema version), then the databases.
  - **`write_backup`** takes each snapshot with `VACUUM INTO` from a read-only connection, so it is consistent even while the app writes, including writes still in the WAL. The snapshots are removed afterwards.
  - **`stage_restore`** refuses, before anything is staged:
    - a wrong magic line, a cut-short file, or an unknown header format or version;
    - a file Zeron does not use, or a duplicated entry;
    - a checksum mismatch, a damaged database (`PRAGMA quick_check`), or a mismatched schema version;
    - data after the last database;
    - a database newer than this build opens (`TooNew`), so a restore can never stop Zeron from starting.
  - **Staging:** staged files are written as `.partial`, synced, and renamed; the pending record is written atomically last.
  - **`apply_pending_restore`** runs at startup, before any database opens:
    - each current database and its `-wal` and `-shm` journals move to `before-restore-<time>/`, and the staged database moves in;
    - a stale journal is never replayed into the restored database;
    - an interrupted run resumes, and the record is removed last.
  - Also `pending_restore` and `discard_pending_restore`.
- **Schema versions exported:** `learning_storage::CURRENT_SCHEMA_VERSION` and `publisher_storage::SCHEMA_VERSION`, so a restore checks against this build.
- **Desktop:**
  - `setup()` applies a pending restore after the old-app import and before opening the databases. A failure stops startup, and the next start retries.
  - **Commands:**
    - `export_backup` writes through a native save dialog, as a `.partial` file renamed on success;
    - `choose_backup_to_restore` picks a file through a native open dialog and stages it;
    - `get_pending_restore`, `cancel_restore`, `restart_app`.
    - Errors: `backup_invalid`, `backup_too_new`, `backup_unreadable`, `backup_failed`, `restore_failed`. No paths cross IPC.
- **UI** (Settings → Backup):
  - "Save backup…" says what the backup holds, and states that course videos, release files, and the signing key are not included.
  - "Restore from backup…" asks for confirmation and explains where the current data goes. Then it offers "Restart now" or "Don't restore".

## Acceptance evidence

- **`data-backup` tests:**
  - a backup of two live WAL databases restores into a data directory holding older data and a stale journal; the old database and journal are kept aside, and a second apply does nothing;
  - an interrupted switch finishes on the next start;
  - a zip file, a cut-short file, a flipped byte, trailing data, a database from a newer build, and a file Zeron does not use are all refused, with nothing staged;
  - a discarded restore leaves nothing to apply.
- **Command test:** what the backup holds is described, and refusal codes carry no paths.
- **Vitest:** saving with and without cancel; the confirmation, staging, restart, and "Don't restore"; the too-new message.
- All repository gates pass.

## Manual verification

1. Settings → Backup → "Save backup…".
2. Add a note to a lesson, then choose "Restore from backup…" and pick the file. Restart.
3. The note added after the backup should be gone, and Zeron's data folder should hold `before-restore-<time>/` with the previous databases.

## Current limitations

- Course packages, downloaded torrents, and release files are not in a backup. After a restore on another computer, the Library asks to locate local course folders, and torrents download again. Publish needs the release files to seed.
- The signing key is backed up separately, from Publish → Identity.
