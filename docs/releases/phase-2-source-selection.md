# Phase 2: backend-only source selection and resource references

Status: complete for attaching, reordering, and detaching lesson resources through a backend-owned file picker with path-free display data.

## Delivered

- `course-publisher::DraftCourse` gains revision-checked `add_resource`, `remove_resource`, and `move_resource`. Resource IDs share the draft-wide uniqueness rule. Detaching removes only the draft reference.
- `publisher-application::PublisherDrafts::attach_source` takes a backend-supplied path, canonicalizes it (resolving symlinks), requires an existing regular file, and records the canonical path only inside the draft snapshot. No bytes are read or copied before packaging. Rejections (`SourceRejected`) use fixed messages that never contain the path.
- Resource kind and declared media type are inferred from the lowercase extension for common video, audio, PDF, Markdown, text, subtitle, image, and ZIP files. Anything else becomes `other` / `application/octet-stream`. Packaging will confirm the declaration later.
- Each resource gets an opaque `resource_` ID and the package path `resources/<id>/<portable file name>`. The file name is reduced to ASCII letters, digits, `.`, `-`, and `_`, has leading dots and trailing dots or spaces removed, is capped at 120 bytes with the extension kept, and is prefixed when it would be a Windows-reserved device name.
- `publisher_application::source_info` returns only the final file-name component and the current size (absent when the file has gone missing).
- `StructureEdit` gains `RemoveResource` and `MoveResource`. Admin `edit_draft_structure` accepts `removeResource` and `moveResource`.
- New Admin command `choose_and_attach_resource(draftId, expectedRevision, lessonId)` opens the native file dialog from Rust through `tauri-plugin-dialog`, as the Learner does for course import. No webview capability was added; the Admin capability remains `core:default` only. Cancelling returns `null` and changes nothing. Rejected sources return `source_rejected`.
- Lesson DTOs now carry `resources: [{ id, kind, fileName, mediaType, byteLength }]` and replace the earlier `resourceCount`. No source path, directory, or package path crosses IPC.
- The draft writer accepts operations that make no change (a cancelled picker) without advancing the revision.
- The Structure panel adds "Attach file" per lesson and lists resources with kind, file name (truncated when long), formatted size or a "File missing" badge, move up/down, and a detach control that states the file itself is kept. Removing a lesson with resources asks for confirmation.
- Fixed: below the `xl` breakpoint the builder grid could grow wider than the window; the single-column track is now `minmax(0, 1fr)`.

## Acceptance evidence

- Domain test: attach, reorder, and detach within a lesson; duplicate IDs and missing lessons or resources are rejected without changing the revision.
- Application unit tests: portable package file names (separators, colons, non-ASCII, leading or trailing dots, reserved device names, length cap with extension kept) and extension classification.
- Application integration tests with real temporary files: classified attachment with canonical source and package path, untouched source bytes, directory and missing-file rejection with path-free messages and no write, detach and reorder keeping files on disk, a file deleted after selection reported as missing by both `source_info` and validation, and symlinked selections recording the resolved file.
- Admin command tests: detail JSON for a real attached file shows `fileName`, `byteLength`, and `kind` without its directory. `removeResource` round-trips and leaves the file in place. A synthetic home-directory source is not leaked.
- Vitest: writer null-result handling; Builder attach through the picker (including cancel without a revision change), missing-file display, resource reorder and detach, and lesson-removal confirmation for lessons with resources.
- Manual verification: in the browser pane with a fake IPC bridge, three attachments and one cancelled pick produced r5, with a truncated long video name, "700 MB", a "File missing" subtitle mirrored by a readiness issue, and "50 KB" for a PDF. The grid-overflow fix was confirmed there (scroll width now equals viewport width). The real debug binary launched with the dialog plugin registered and ran until stopped.
- All repository gates pass.

## Current limitations

- The native file dialog itself was not exercised by automation or by hand in this slice; its Rust call mirrors the Learner's folder picker. The attach path below the dialog is covered by tests with real files.
- One file per pick; multi-select and drag-and-drop from the file manager are not supported yet.
- Kind and media type cannot be overridden in the UI, and language and duration are not editable yet.
- Media inspection (container, codec, duration, `moov` placement) is the next slice; extension-based classification is only a declaration.
- Source changes after selection (edits, replacement) are not detected until packaging hashes the bytes.
