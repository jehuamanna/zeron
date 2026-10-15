# Phase 6: notes anchored to lessons and video moments

Status: complete. Learners can write rich-text notes for a lesson, or at a moment in its video. A timestamped note jumps the video to its moment. Notes are never overwritten by a stale edit. See [ADR 0009](../decisions/0009-learner-notes.md).

## Delivered

- **`learning-domain`:**
  - `Anchor` has three kinds: lesson, video time, or PDF page with an optional region. It parses strictly, validates ranges, and serializes with `to_json` / `from_json`.
  - `NoteBody` validates a Tiptap document and extracts its plain text.
- **`learning-storage` schema v9:**
  - The `notes` table is indexed by course, lesson, and creation time. Ties sort by insertion order.
  - `insert_note`, compare-and-swap `update_note` and `delete_note` (returning `NoteWrite::{Saved, Conflict, Missing}`), `note`, and `notes(course, lesson?)`.
- **`learning-application`:**
  - `create_note`, `update_note`, `delete_note`, and `notes`. Anchors are checked against the course manifest.
  - `NoteChange::{Saved, Conflict, NotFound}`.
  - New errors: `LibraryError::Note` (invalid anchor or document) and `LibraryError::NoteTarget` (unknown lesson or resource).
- **Learner commands:**
  - `list_notes`, `create_note`, `update_note { noteId, expectedRevision, anchor, body }`, and `delete_note { noteId, expectedRevision }`.
  - Changes return `{ outcome: "saved" | "conflict" | "notFound" }`; a conflict carries `currentRevision`.
  - Errors are `invalid_input`, `note_target_invalid`, and `library_write_failed`; none carries a path.
- **UI:**
  - The **Tiptap 3.32.0** editor (`@tiptap/react`, `@tiptap/pm`, `@tiptap/core`, `@tiptap/starter-kit`; MIT; exact pins) is the new dependency.
  - Each lesson has a "Notes" button, and the notes panel follows the lesson being played.
  - "Add note at m:ss" takes the player's current time; "Add lesson note" is also available. A timestamp chip seeks the playing video, or opens that video starting at the note.
  - Notes can be edited and deleted. On a conflict the panel keeps the learner's text and offers "Load latest".
  - `LessonPlayer` exposes `currentTime` and `seek` through a ref handle.

## IPC and storage compatibility

- **IPC:** additive. There are four commands and new types: `Note`, `NoteAnchor`, and `NoteChange`.
- **Storage:** migrating from v8 to v9 adds one table. Older builds refuse v9.

## Acceptance evidence

- **Domain tests:** strict anchor parsing and the JSON round trip; range checks for timestamps, pages, and regions; document validation (non-documents, wrong types, nesting over 64 levels, over 256 KiB); plain-text extraction.
- **Storage test:** a course is required and IDs are unique; updates from a stale revision are conflicts and leave the note unchanged; deletes are compare-and-swap; notes survive a re-import.
- **Application tests** (`notes_are_anchored_only_to_real_targets`, `notes_change_only_from_their_current_revision`): lesson and video anchors are accepted; unknown lessons, wrong-kind or foreign resources, negative times, and invalid bodies are refused; a stale update is a conflict and the saved text is unchanged; ordering; compare-and-swap delete; not found.
- **Command test:** the tagged outcome shape, strict anchor parsing, and error codes.
- **Vitest:**
  - The panel lists notes and their anchors; a timestamp chip seeks; a note can be added at the current time or for the whole lesson.
  - A conflict keeps the draft, disables saving, and offers "Load latest"; deletes send the revision.
  - The real Tiptap editor renders a stored document read-only, shows `<img onerror>` as text without creating an element, and falls back to an empty document for unreadable content.
  - The lesson "Notes" button opens its panel.
- **Real window** (the user's "Systems Design Interview Questions" course, Learner driven by WebDriver):
  - Playing "Design Slack" moved it from not started to in progress.
  - A note typed into the real Tiptap editor at 5:01 was saved. After seeking to 0:20, its "Go to 5:01" chip returned the video to 5:02.
  - Watching past 90% marked the lesson completed. The course and the library card then read "1 of 13 lessons complete".
  - After restarting the Learner, the note and the completed status were still there.
- All repository gates pass.

## Current limitations

- PDF page notes are modelled and validated, but creating them waits for the PDF viewer slice.
- Notes are not searchable yet; that is the search slice.
- There is no note history; the newest saved revision is the note.
- The Learner's main bundle grew to about 742 kB (232 kB gzipped) with Tiptap. The PDF and drawing slices load their libraries lazily so they don't grow it further.
