# Phase 7 (pulled forward): drag and drop into the Builder

Status: complete, with one manual check pending (below). This is sub-slice 4 of the Admin course management work. Dropping folders onto an open draft adds one module per folder, with one lesson per video, audio, or PDF file in natural name order. Files dropped onto a lesson attach to it. See [ADR 0007](../decisions/0007-builder-drag-and-drop.md).

## Delivered

- **`course-publisher`:** `DraftCourse::import(expected_revision, modules, lesson_resources)` adds whole modules and attaches resources to one lesson in a single revision step. It refuses duplicate or used IDs and missing lessons, and leaves the draft unchanged on failure.
- **`publisher-application::drops`:**
  - `plan_drop` and `DropLimits` cover bounds, one folder level, no symbolic links, skipping hidden entries, `SkipReason`, and natural ordering (`natural_cmp`).
  - `clean_title` handles names like `10 Design Tinder.mp4.mp4` → "Design Tinder".
  - `PublisherDrafts::apply_drop` reports what was added and what was left out.
  - `DropInbox` holds opaque tokens, with capacity and expiry.
- **Admin Tauri:**
  - `WindowEvent::DragDrop` is handled in Rust. It emits `builder-drag { active }` and, after scanning off the event loop, `builder-drop` (a path-free summary).
  - Commands `apply_drop { draftId, expectedRevision, token, target: { lessonId } }` and `discard_drop { token }`. Error codes: `drop_expired`, `revision_conflict`, `invalid_input` (`field: "drop"`), `entity_not_found`.
- **Builder UI (`DropImport`):**
  - A drop hint appears while dragging.
  - A review panel lists the modules to add (expandable lesson lists with file names), the target lesson for loose files (preselected from the drop point), and the items left out with readable reasons.
  - "Add to course" goes through `draftWriter`, so conflicts pause editing as usual. "Dismiss" discards the token, and a new drop replaces and discards the previous one. The result summary shows counts.

## IPC compatibility

The change is additive: two events, two commands, and new TypeScript types (`DropSummary`, `DropOutcome`, `SkippedEntry`, `SkipReason`). Lesson rows carry `data-lesson-id`.

## Acceptance evidence

- **Domain test:** imports are atomic and take one revision.
- **Application tests** (`tests/drops.rs`):
  - natural ordering and title cleanup;
  - a planned tree with nested folders, hidden files, file and folder symlinks, unsupported files, a missing path, and a folder with no lessons, all reported by relative name;
  - entry, file, and folder limits;
  - one-revision application with lessons in order, PDF and video kinds, sanitized package paths, absolute canonical sources, and a vanished file skipped;
  - loose files without a lesson, and empty drops;
  - inbox tokens, capacity, and expiry.
- **Real courses** (`real_course_folders_become_modules`, `--ignored`, run on 2026-10-16 with `ZERON_DROP_FIXTURES` set to the user's two folders):
  - `algoe-sixquevideo` became 13 lessons in numeric order, from "Design A Code-Deployment System" to "Design The Twitch API", with the `.mp4.mp4` names cleaned up.
  - `algoexpert-sdfshortcourse` became 20 lessons, `lesson1` … `lesson20` in natural order.
  - `Design Fundamentals.txt` was reported as `unsupported_type`. Every lesson had a duration read from its MP4 header.
- **Command tests:** summaries and outcomes contain no scratch paths, tokens are single-use (`drop_expired`), a stale revision is a conflict, and targets reject unknown fields.
- **Vitest:** the drag hint; the drop review with lesson preselection from the drop point; apply as one revision with the summary; the required lesson for loose files; dismissed and replaced drops discarded; the expired-drop message; the conflict banner; the empty-drop state.
- All repository gates pass.

## Manual verification (pending)

Native drag gestures cannot be driven by WebDriver. To check by hand:

1. Open a draft in the Admin app.
2. Drag `algoexpert-sdfshortcourse` from the file manager onto the window. The hint should appear, then the review should list 20 lessons and the `.txt` file as left out.
3. Choose "Add to course". The module should appear with its lessons in order.
4. Drag a PDF onto a lesson row. The panel should preselect that lesson.

## Current limitations

- Only one folder level is imported. Nested folders are listed as left out.
- Drops made while no draft is open are ignored, and their tokens expire.
- `Design Fundamentals.txt`-style title lists are not used to rename lessons.
