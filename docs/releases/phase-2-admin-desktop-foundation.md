# Phase 2: Admin Desktop foundation

Status: complete for creating, listing, and reopening durable publisher drafts.

## Delivered

- `apps/admin-desktop` is an independently packaged Tauri application, "Zeron Publisher", with identifier `app.zeron.publisher`, development port `1421`, a strict CSP without media origins, and a `core:default`-only capability.
- Tauri setup composes `publisher-storage` against an app-data `publisher.sqlite3` file behind `Arc<Mutex<_>>`; storage work runs on blocking workers.
- Thin `list_drafts`, `create_draft`, and `get_draft` commands translate drafts into path-free DTOs. Command bodies are Tauri-independent functions so they are tested against in-memory storage.
- Draft and course IDs are opaque `draft_`/`course_` prefixes over 128 random bits from the OS generator.
- A new draft starts at revision zero with version `0.1.0`, "Untitled course", "Local publisher", language `en`, and no modules.
- `get_draft` returns course metadata, ordered module/lesson IDs and titles, and per-lesson resource counts; it never returns source paths, package paths, or resource media details.
- The React Courses and Builder views list drafts, create a draft, and open it in the builder shell.
- Root `pnpm test`, `typecheck`, and `build` now run against every app.

## Acceptance evidence

- Rust unit tests cover new-draft defaults, opaque ID shape and uniqueness, create/list/get round trips through storage, missing-draft `None`, and DTO serialization that omits `sourcePath`, `packagePath`, and absolute paths while counting resources.
- Vitest covers path-free draft listing and the create-then-open builder flow.
- Manual verification (Linux): running the debug binary against the Vite dev server opened the window, created `~/.local/share/app.zeron.publisher/publisher.sqlite3` at schema version 1, and remained running until stopped.
- All repository gates pass: rustfmt, Clippy with warnings denied, workspace tests, both app test/typecheck/build runs, and `git diff --check`.

## Current limitations

- Drafts cannot yet be edited, renamed, deleted, or duplicated; module/lesson/resource editing is the next slice.
- Distribution and Settings navigation entries are placeholders.
- The Admin app reuses the Learner icon until product artwork exists.
- The window's create/open flow was exercised by component tests; the native-window click path was not automated.
- `get_draft` reads the list query to recover the last-saved timestamp; a single-row lookup is deferred until draft counts make it matter.
