# Phase 1 completion note: course navigation and package recovery

Date: 2026-10-15

## Delivered scope

- Added storage migration version 2 for backend-only verified package locations.
- Preserved non-UTF-8 Unix paths and native Windows UTF-16 paths without lossy conversion.
- Updated verified import so the canonical package root and course snapshot commit together.
- Added backend-only resource resolution by stable course/resource IDs for the upcoming media registry.
- Added a dedicated, path-free course-detail IPC contract with ordered modules, lessons, and resources.
- Added Zustand-owned course selection and a Learner course workspace/navigation UI.

## Acceptance evidence

The full Rust and frontend verification suite passes. The file-backed restart test now proves resource resolution after recovery. Three UI tests cover populated library, empty library, and course-outline navigation without path disclosure.

## Manual verification

Import a course, select its card, inspect the ordered module/lesson/resource outline, restart the app, and repeat. Confirm the frontend payloads contain stable IDs and metadata but no local package root.

## Security review

Package roots are stored only in the private SQLite adapter using an OS-native byte encoding. They are resolved only by the application service and are absent from IPC DTOs. Resource resolution requires both a stored validated manifest and a stable resource ID; arbitrary relative paths are not accepted from the frontend.

## Known limitations

- The resolved path is not yet registered with a media gateway, so resource pills are informational.
- Relocated or deleted package roots currently surface as playback/open failures; startup reconciliation will add explicit missing/moved states.
- Migration rollback/fault injection remains a hardening task.

## Next dependency-ordered slice

Introduce a loopback-only complete-file media gateway with opaque resource handles, per-session authorization, strict single-range parsing, bounded reads, and shutdown. Use it for local video playback without exposing filesystem paths. The same gateway boundary will later consume verified librqbit streams.
