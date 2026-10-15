# Phase 2: publisher domain foundation

Status: complete as the policy foundation for Admin Desktop.

## Delivered

- `course-publisher` owns editable course, module, lesson, and source-resource drafts without depending on Tauri, React, SQLite, or a torrent engine.
- Draft resources keep backend-only source paths separate from requested package-relative paths.
- Revision-checked metadata and reorder operations reject stale autosave clients rather than applying last-write-wins updates.
- Validation reports stable issue codes and editor field locations for required fields, empty structure, duplicate stable IDs, duplicate package paths, and missing/non-file sources.
- The durable publishing state machine requires each validation, packaging, torrent generation, verification, activation, and seeding stage in order.
- Failed or cancelled mutable attempts may explicitly restart from a draft; published releases cannot return to mutable stages.

## Acceptance evidence

- Reorder tests prove stable IDs survive ordering changes and stale revisions are rejected.
- Draft validation tests cover missing source files, empty modules, and a valid regular source.
- Workflow tests cover the complete happy path, skipped-stage rejection, terminal immutability, and recoverable retry.

## Current limitations

- Durable Admin draft storage, autosave IPC, and the Admin Desktop shell are the next slice.
- Packaging, hashing, manifest emission, media probing, torrent generation, and release activation are intentionally not claimed by this foundation.
- Draft source paths are backend-owned records and must be mapped to path-free DTOs before an Admin React client is connected.
