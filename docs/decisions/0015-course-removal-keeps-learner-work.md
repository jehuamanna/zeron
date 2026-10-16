# ADR 0015: Removing a course keeps the learner's work

Status: accepted (2026-10-16)

## Context

Learners asked to delete courses from the library. Every learner-data table (`notes`, `lesson_drawings`, `lesson_progress`, `playback_positions`, and more) references `courses(course_id)` with `ON DELETE CASCADE`, so deleting a course row would also delete everything the learner wrote and drew. The user decided (2026-10-16) that:

- removing a course keeps notes, highlights, sketches, and progress, and they come back if the same course is imported again;
- for a local folder import, Zeron removes only its own record and never touches the folder the learner picked.

Deletion and path handling are security-review triggers (AGENTS.md).

## Decision

1. **Soft removal in storage (schema 15).**
   - `courses` gains `removed_at`. A removed course keeps its row, so nothing cascades.
   - In one transaction, removal deletes what tied the course to content:
     - its location, torrent source, download commitments, and resources;
     - its outline and text index (notes stay indexed);
     - its signature and publisher pin, as ADR 0014 already states for removal.
   - It keeps notes, drawings, progress, playback positions, and manifest history.
   - The library list, course lookup, manifests, search, and start-up torrent restore all ignore removed courses. Writing new notes to a removed course therefore fails as "not in the library".
   - `kept_course_manifest` and `removed_courses` let the notebook show a removed course's notes in lesson order, read-only.
2. **Re-import.** The import upsert sets `removed_at = NULL`. A re-imported course returns with its kept work, and its notes are searchable again.
3. **Files.**
   - A local folder import never had files in Zeron's data, so its folder is never touched.
   - A torrent course's download directory belongs to Zeron and is deleted only when all of these hold:
     - it is exactly `courses_root/<info hash>`, and the hash parses;
     - it is a real directory, not a link;
     - no other stored course uses the same torrent.
   - Order, so a crash cannot lose track:
     1. Stop the transfer and forget its sharing and pause choices (`TransferRouter::forget`), so a re-import starts private.
     2. Rename the directory to `<hash>.deleting`.
     3. Mark the course removed. If that fails, the rename is undone.
     4. `remove_dir_all`, which removes links inside rather than following them.
   - At start-up, `finish_interrupted_removals` deletes any leftover `<hash>.deleting` that is a real directory, and nothing else.
4. **Playback.** If the removed course is playing, its media capability URL is revoked first, so nothing keeps reading files that are about to go.
5. **Interface.** "Remove from library…" is in each course card's menu and in the open course's actions menu. The confirmation says exactly what goes (for torrents, the downloads and their size) and what stays. Errors never carry paths.

ADRs 0009 and 0010 said notes and drawings cascade "on course removal". With this ADR, removing a course no longer deletes the course row, so they are kept.

## Consequences

- Removed courses' rows and learner data stay in the database until a future "delete my notes too" action exists. They are small, and the notebook shows them.
- A crash between the rename and marking the course removed leaves the course listed while its download is gone. The next start deletes the `.deleting` directory, and the course re-downloads its outline. The learner asked to remove that course, so nothing they wanted is lost.
- Tests:
  - storage `removed_courses_keep_learner_work_and_come_back_on_reimport`, plus both schema-upgrade tests;
  - `course_removal.rs`: setting aside, restoring, refusing foreign or shared or linked directories, never following links, and start-up recovery;
  - router `forgetting_a_removed_course_stops_it_and_its_choices`;
  - Library UI tests for both removal entry points and the read-only notebook.
