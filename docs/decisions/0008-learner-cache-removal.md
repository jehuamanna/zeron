# ADR 0008: Removing streamed lesson data from the Learner

Status: accepted (2026-10-16)

## Context

Streaming leaves lesson pieces on disk below `<app data>/courses/<info hash>/`. Phase 5 needs a cache policy, so learners can free space and set a cap on streamed lessons. Deletion is a security-review trigger. The constraints:

- Only learner-downloaded data may go. Publisher releases, `course.json`, and lessons the learner keeps offline (`phase-5-download-commitments.md`) must never be removed.
- librqbit keeps pieces it has verified in memory. If a file disappeared under a running torrent, librqbit would still believe its pieces were present.

## Decision

1. **Scope.**
   - `CourseCache` removes only the files of lesson resources that are not committed, for one torrent course.
   - The course's stored content root must equal `courses_root/<info hash>`, the only place the import service writes. Otherwise nothing is removed.
   - `course.json` is never a candidate.
2. **Paths.**
   - Candidate paths come from the stored torrent's file table, which was validated at import. They are re-checked to be relative, with normal components only and no `\`, `:`, or NUL.
   - Removal walks the path with `symlink_metadata`. The content root and every intermediate component must be real directories, and the last component a regular file; anything else is skipped.
   - `remove_file` never follows the final component.
3. **Engine coordination.** If the course is active, its transfer is stopped, the files are removed, and the transfer is added again with the stored selection and pause state. librqbit's initial check then sees the missing pieces. Streams reopen a fresh engine reader per request (`open_lesson_stream` waits for the re-added torrent), so a course that is playing recovers.
4. **Policy.**
   - "Remove download" acts on one lesson and "Free up space" on all of a course's non-kept lessons.
   - An optional cap (`cache_limit_bytes`, at least 256 MiB) removes the least recently played non-kept lessons first; a lesson never played counts as oldest.
   - The cap is applied after startup restore, when it changes, and after a lesson opens. Enforcement never touches the course being played, so its transfer is not restarted under the player.
5. **Reporting.** Results list the removed and refused (kept) resource IDs and the verified bytes freed. Errors carry no paths.

## Consequences

- Kept lessons, `course.json`, and publisher data are never removed. Files reached through planted links are never touched; tests cover a link put in place of a lesson file.
- Removing a lesson also invalidates the pieces it shares with its neighbours at file boundaries. Kept neighbours download those pieces again; unkept ones simply have a little less data.
- librqbit may re-create a removed file as an empty or sparse file when the transfer is added again. The space is still freed.
- There is a time-of-check to time-of-use gap: a process that can already write inside the app's data directory could swap an intermediate directory for a link between the check and the removal. Such a process already controls the learner's data, so this is accepted.
