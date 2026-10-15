# Phase 1 completion note: Learner Desktop foundation

Date: 2026-10-15

## Delivered scope

- Added independently packaged `apps/learner-desktop` using Tauri 2, React 19, strict TypeScript, Vite, Tailwind CSS, shadcn-style UI primitives, Lucide, TanStack Query, and Zustand as a pinned workspace.
- Added a restrictive explicit CSP and a minimum Tauri capability set with no frontend filesystem plugin.
- Added a native Rust folder picker so React never supplies or receives a local path.
- Added thin, version-shaped IPC DTOs for readiness, path-free course listing, and verified local import.
- Routed blocking database, dialog, and hashing work through blocking workers instead of the native main thread or async runtime workers.
- Added library loading, empty, populated, importing, and recoverable error UI states.
- Added a generated Zeron application icon at `apps/learner-desktop/src-tauri/icons/icon.png`.

## Acceptance evidence

The complete Rust workspace passes formatting, strict Clippy, compilation, and all eighteen Rust tests. The frontend passes strict TypeScript, two Vitest/jsdom UI tests, and a production Vite build. Native Linux Tauri compilation succeeds against WebKitGTK 4.1 on this host.

## Manual verification

Run `pnpm --filter @zeron/learner-desktop tauri dev`, choose a valid course directory, verify that the course card appears without revealing its path, close the app, reopen it, and confirm recovery from the file-backed library database. Select an invalid or tampered package and confirm the library remains unchanged.

## Security review

- The frontend receives no arbitrary filesystem capability and cannot pass a path into an import command.
- Native folder selection yields a backend-local path consumed only by the existing validation use case.
- IPC returns dedicated DTOs rather than database, filesystem, or library implementation types.
- The CSP denies objects, frames, and arbitrary network/media origins. Loopback media permissions are intentionally absent until the authenticated gateway phase.
- Full error text is currently returned for local diagnostics; production hardening must classify/redact all transitive errors before release.

## Known limitations

- The current course cards do not yet open lesson navigation or a complete-file player.
- Import cancellation is implemented in the use case but not yet exposed as a UI job because the current command is one awaited operation.
- Native packaging was compiled but installers were not produced or smoke-tested in this slice.
- The generated icon is a PNG source only; platform-specific icon bundles are deferred to release packaging.
- UI tests mock IPC. Native command integration is covered indirectly through application-service tests, not a WebDriver run.

## Next dependency-ordered slice

Persist backend-only package locations, add a course-detail/lesson navigation DTO, and implement a narrowly registered complete-file media resource route. Then verify local HTML5 playback and resume position before beginning Admin Desktop publishing work.
