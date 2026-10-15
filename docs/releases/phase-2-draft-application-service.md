# Phase 2: draft application service and metadata autosave

Status: complete for course-metadata editing with revision-conflict reporting and live draft validation.

## Delivered

- New `publisher-application` crate owns the Admin draft use cases: create (opaque IDs and defaults moved out of the Tauri layer), list, load, metadata autosave, and validation. It depends only on `course-publisher`, `publisher-storage`, `getrandom`, and `thiserror`.
- `PublisherStorage::stored_draft` loads a snapshot and its save time in one query, replacing the list scan used by `get_draft`.
- Metadata autosave uses the domain revision check and the storage compare-and-swap; both stale paths surface as `RevisionConflict { expected, actual }` and never overwrite the newer save.
- The logical course ID is not part of the edit contract and cannot be changed; unknown IPC fields such as `courseId` are rejected.
- Autosave input is bounded and checked before any write: title/publisher 200 characters, version 64, language 63, category/license 100, description 10,000; at most 32 tags (64 characters each) and 32 prerequisites (200 characters each); single-line fields reject control characters; the description allows only newline and tab. Blank optional fields become absent; list entries are trimmed, emptied entries dropped, and duplicates removed.
- New Admin commands `update_draft_metadata` and `validate_draft`. Command errors carry a stable `code`, plus `currentRevision` for `revision_conflict` and `field` for `invalid_input`.
- The Builder has a Course details form with a 700 ms debounced, serialized autosave and an explicit status: saved with revision, unsaved, saving, conflict, or rejected with retry. A conflict disables the form until the publisher chooses "Discard my edits and load latest".
- Publishing readiness lists blocking draft validation issues by field and refreshes after each save.
- Builder selection lives in a Zustand store; persisted draft data stays in TanStack Query.

## Acceptance evidence

- `publisher-application` integration tests: create defaults and opaque IDs, autosave revision advance with stable course ID and normalization, stale-autosave conflict preserving the first save, bounded/control-character rejection without writing, missing drafts, validation of saved drafts, and file-backed recovery after reopen.
- `publisher-storage` test for single-query snapshot and save-time loading.
- Admin Tauri command tests: create/list/get round trip, next-revision saves, conflict JSON with `currentRevision`, invalid-input JSON with `field`, rejection of an injected `courseId`, path-free validation issues, and resource counts without source or package paths.
- Vitest: debounced edits coalesce into one save at the owned revision and the next save uses the new revision; conflicts stop autosave and offer reload; rejected input marks the field and retries; the builder shows readiness issues.
- Manual verification: in the browser pane against the Vite dev server with an in-page fake of the Tauri IPC bridge, typing a title and clearing the publisher name produced one save from r0 to r1 and a readiness issue for the publisher name. An external revision bump then made the next edit report "Changed elsewhere (r2)"; reloading showed the external title at r2 without another write. No console errors.
- All repository gates pass.

## Current limitations

- Only course metadata is editable; module, lesson, and resource editing is the next slice.
- A conflict offers reload-and-discard only; there is no field-level merge of the publisher's unsaved edits.
- Version, language, and identifier format checks run later with manifest generation; the readiness panel currently reports required fields, structure, uniqueness, and source existence.
- Autosave timing is client-side; an app exit within the debounce window loses that pending edit.
- The manual check used a fake IPC bridge in a browser because the native webview cannot be driven by the available tooling; the Rust command path is covered by tests against real SQLite.
