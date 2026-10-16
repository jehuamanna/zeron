# Phase 7: suggestions for moving annotations to a new course version

Status: complete. When a newer version of a course replaces the one in the Library and renames, replaces, or removes lessons, the course page lists the lessons whose notes, drawing, or progress the new version no longer shows. Each comes with a suggested lesson where one matches. Nothing moves until the learner chooses "Move here".

## Delivered

- **`learning-domain::migration`** (pure):
  - `suggest_lesson` picks the new lesson sharing the most files with identical contents (SHA-256). Otherwise it picks the only lesson with the same title, ignoring case and spacing. A tie gives no suggestion.
  - `resource_map` maps an unchanged resource ID to itself, and otherwise maps a resource to the one with identical contents.
  - `remap_anchor` follows that map. A timestamp or page whose file changed becomes a lesson-level anchor, because the same second or page may no longer mean the same place.
- **`learning-storage` schema v12:**
  - **Manifest history.** `course_manifest_history` keeps the manifests of the versions a course replaced, at most `KEPT_PREVIOUS_MANIFESTS` (5) per course, newest first. Re-importing a version removes it from the history.
  - **Search.** The new trigger `notes_search_move` keeps a moved note searchable under its new lesson.
  - **The move.** `move_lesson_annotations` works in one transaction:
    - notes move with their new anchors, and their revisions increase, so an open editor reloads rather than overwrites;
    - the drawing (with its images), lesson progress, and the playback positions of mapped files move only where the target has none.
  - `previous_manifests` and `drawing_lessons` are new.
- **`learning-application`:**
  - `CourseLibrary::annotation_moves(course_id)` covers lessons with annotations that the current version lacks, and kept lessons whose notes point at files the lesson no longer has (reason `same_lesson`). Each entry gives counts and how many notes would be widened.
  - `CourseLibrary::move_annotations(course_id, from, to)` accepts a suggestion or any lesson the learner picks, and refuses a target the current version does not have.
- **Commands:**
  - `get_annotation_moves { courseId }` adds the suggested lesson's title.
  - `move_annotations { courseId, fromLessonId, toLessonId }` returns what moved.
  - Errors use the note codes `note_target_invalid` and `library_write_failed`.
- **UI** (`AnnotationMovesCard`, on the course page):
  - The card appears only when something needs a place.
  - Each row shows the old lesson's title and version, what is on it, the suggestion with its reason and any widening, and a lesson picker preselected to the suggestion. "Move here" moves the annotations.
  - A summary reports what moved, including a drawing kept apart.

## Compatibility

- **Storage:** migrating v11 to v12 adds the history table and the trigger. History starts with the first version replaced after the upgrade, so a course updated before this build has no previous manifest. Its orphaned notes are still listed, without a suggestion, and can be moved by hand.
- **IPC:** additive, two commands.

## Acceptance evidence

- **Domain tests:** identical files beat titles, unique-title fallback, ties and empty titles give no suggestion, resource mapping by ID then contents, and anchors widened only when their file changed.
- **Storage tests:**
  - history is newest first, bounded at five, and drops a re-imported version;
  - a move carries notes (revisions increase, search follows), the drawing with its image, completed progress, and a playback position;
  - a second move onto a lesson with its own drawing, progress, and position overwrites none of them.
- **Application tests** (`tests/annotation_migration.rs`, real packages imported from disk):
  - Version 1.0.0 has four lessons with notes, a drawing, progress, and a position. Version 1.1.0 changes them as follows:
    - Intro → Introduction keeps the same video, suggested as `same_content`;
    - "Caching" → "  caching " with a re-recorded video, suggested as `same_title` with both notes widened;
    - a removed lesson, with no suggestion;
    - a kept lesson whose video changed, as `same_lesson`.
  - Each move gives the expected anchors and counts, the list empties, and an unknown target or course is refused.
  - A drawing never replaces the target lesson's own.
- **Command test:** an unknown course gives `note_target_invalid`.
- **Vitest:** hidden when empty, suggestions explained and preselected, a manual pick with the report, the report wording, and a failed move.
- All repository gates pass.

## Manual verification

1. Import version 1 of a course from a folder and add notes to two lessons.
2. Publish version 2 with one lesson renamed (new lesson) but the same video, and import it.
3. Open the course. The card should suggest the renamed lesson as "Has the same file". Move it, and the notes should appear on the new lesson at the same timestamps.

## Current limitations

- Only lesson-level matching is offered; a note is never split across lessons.
- A kept resource ID whose file changed keeps its timestamps, since notes made before and after the update cannot be told apart.
- Hidden annotations stay until moved; there is no "discard" action, by design.
