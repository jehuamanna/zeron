# Phase 2: module and lesson structure editing

Status: complete for adding, renaming, removing, and reordering modules and lessons with keyboard-accessible controls.

## Delivered

- `course-publisher::DraftCourse` gains revision-checked `add_module`, `rename_module`, `remove_module`, `add_lesson`, `rename_lesson`, and `remove_lesson`, alongside the existing moves. New entities must use IDs not already present anywhere in the draft (`EditError::DuplicateId`). Removal drops only draft references; source files are never touched.
- `publisher-application::PublisherDrafts::edit_structure` applies one `StructureEdit` per revision. Module and lesson IDs are generated as opaque `module_`/`lesson_` values, titles share the 200-character single-line bound, and missing targets or out-of-range moves are reported as `EntityNotFound` / `InvalidPosition`.
- Admin command `edit_draft_structure` accepts a tagged, camelCase edit (`addModule`, `renameModule`, `removeModule`, `moveModule`, `addLesson`, `renameLesson`, `removeLesson`, `moveLesson`). Unknown edit types and extra fields, including caller-chosen IDs, are rejected. New error codes are `entity_not_found` and `invalid_position`.
- The Builder now owns one serialized draft writer per editing session. Metadata autosave and structure edits go through it, so each write carries the revision returned by the previous one and they can never conflict with each other.
- A conflict from any editor shows one banner, pauses every editor, and offers "Discard my edits and load latest". The current draft revision is shown above the editors.
- The Structure panel lists modules and lessons in order with inline title editing (Enter or blur commits, Escape reverts), move up/down buttons, and Alt+↑/↓ while editing a title. Focus stays on the moved item, and moves, additions, and removals are announced through a polite live region.
- Removing a module that still has lessons, or a lesson that has resource references, asks for inline confirmation. Focus moves to Cancel, Escape cancels, and focus returns to the remove button. Empty modules and lessons are removed immediately.

## Acceptance evidence

- Domain tests: one revision per add/rename/remove, stale edits leave the draft unchanged, IDs are unique across modules and lessons, missing targets are reported by entity, and lesson reordering is bounded.
- Application tests: outline building and reordering persisted across reload from storage, nested removal, metadata and structure sharing one revision sequence with stale writers rejected, and rejection of missing targets, bad positions, and multi-line or overlong titles without writing.
- Command tests: tagged-input round trip, `entity_not_found` / `invalid_position` codes, and rejection of unknown edit types and caller-supplied IDs.
- Vitest: writer serialization, revision hand-off, and refusal after conflict. Builder tests against a stateful fake backend cover add, rename, button and Alt+Arrow reordering with announcements, confirmed module removal with focus handling, immediate empty-lesson removal, metadata autosave interleaved with a structure edit, and the shared conflict banner.
- Manual verification: in the browser pane with a fake Tauri IPC bridge, two modules and two lessons were added, a module was renamed and moved with Alt+↓ (rename at r4, move at r5, focus followed the module), a non-empty module was removed using only the keyboard (Enter, Escape, Enter, Shift+Tab, Enter), and a forced conflict disabled every editor behind the banner. Two issues found this way were fixed and covered by tests: the move announcement used the pre-rename title, and focus fell to the page body when removal confirmation opened.
- All repository gates pass.

## Current limitations

- Lessons move only within their module; moving a lesson to another module requires remove and re-add.
- No pointer drag-and-drop yet. Reordering uses buttons and keyboard shortcuts.
- After a module or lesson is removed, focus returns to the page body rather than a neighbouring item.
- Resources cannot yet be added, reordered, or removed individually; that arrives with backend-only source selection.
- Lesson estimated duration is not editable yet.
