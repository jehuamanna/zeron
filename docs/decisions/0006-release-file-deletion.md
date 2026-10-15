# ADR 0006: Deleting an archived release's files

Status: accepted (2026-10-16)

## Context

Releases are immutable, and AGENTS.md forbids removing release files casually. Publishers still need to reclaim disk space from versions they no longer distribute. The user decided on 2026-10-16 that deletion is allowed only for archived releases, needs a typed confirmation, never follows symbolic links, and must stay inside the releases folder. Deletion is a security-review trigger, and a crash can interrupt it at any point.

## Decision

1. **Eligibility.**
   - Only a release whose status is `archived` qualifies. Archiving (sub-slice 2) has already stopped seeding and removed the seeding intent, and an archived release is never seeded again.
   - Published and superseded releases are refused with `ReleaseNotArchived`.
   - The release id must pass `release_directory` (`release_<32 hex>`). Anything else is reported as an unknown release.
2. **Consent in the backend.** `delete_release_files` requires `confirmation` to equal the release's course version, ignoring surrounding whitespace. The check runs in Rust, so a stray IPC call cannot delete files. The UI asks the publisher to type the version.
3. **Record first, then remove.** Schema v4 adds `releases.files_state`, which is one of `present`, `deleting`, or `deleted`. Deletion runs in this order:
   1. Set `files_state = 'deleting'` (only for archived releases).
   2. If `<root>/<release_id>` is a real directory, re-check, after resolving every link, that it sits directly under the canonical releases root.
   3. Rename it to `<release_id>.deleting`. If something already sits at that name, from an earlier interrupted attempt, remove it first.
   4. Remove `<release_id>.deleting`. A directory is removed with `std::fs::remove_dir_all`, which does not follow symbolic links inside the tree. A link or file at the top level is unlinked, not followed.
   5. Set `files_state = 'deleted'`.
4. **History is kept.** The row stays, with its magnet, info hash, and version. `insert_release` keeps refusing that version, so a version is never reused, as ADR 0003 requires.
5. **Recovery.** At startup, after `recover_releases`, `recover_release_deletions` repeats steps 3.ii–3.v for every `deleting` row. Steps 3.ii–3.v are idempotent, so a crash before or after the rename, or midway through the removal, converges to `deleted`. A recovery failure does not block startup: the row stays `deleting`, the UI offers a retry, and the next start tries again. `recover_releases` ignores `.deleting` names.

## Consequences

- No window exists in which the database says `present` while the files are partly gone.
- A symbolic link planted inside or in place of a release directory is unlinked, never followed. Files outside the releases root are never touched. Tests cover both cases.
- A deleted release cannot be seeded, exported, or re-verified. Its magnet stays visible as history.
- Read-only release files are removed through their writable parent directories on Unix. Behaviour on Windows, where read-only files resist deletion, relies on `remove_dir_all` and has not been tested yet.
- Undoing a deletion is impossible by design.
