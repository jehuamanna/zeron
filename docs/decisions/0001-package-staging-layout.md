# ADR 0001: Publisher package layout, canonical manifest bytes, and staging attempts

Status: accepted (2026-10-15)

## Context

Phase 2 packaging turns a mutable draft into an immutable package that is later described by BitTorrent v1 metainfo. Torrent identity depends on every file path and byte in the package, including `course.json`. Publishers rename modules, lessons, and files freely while editing. Packaging copies potentially very large media, so interruptions and crashes are expected. The course-manifest specification lists `modules/<slug>/<file>` paths only as an example.

## Decision

1. **Resource paths are scoped by resource ID.** When a source is attached, it receives the package path `resources/<resource_id>/<portable file name>`. The file name is reduced to ASCII letters, digits, `.`, `-`, and `_`, and guarded against hidden-file, trailing-dot, length, and Windows-reserved-name problems. Because opaque resource IDs are unique and stable, paths never collide or case-fold together, and renaming a module or lesson does not change any path or hash.
2. **The manifest is generated from the draft, never hand-edited.** Module and lesson `position` values come from draft order. `course.json` is serialized as pretty-printed JSON in struct field order, with a trailing newline. The same draft and the same source bytes produce byte-identical `course.json` and package trees.
3. **The manifest is validated twice.** It is checked with placeholder digests before any copy, so metadata errors fail fast, and again with real lengths and SHA-256 digests before it is written.
4. **Staging copies and never mutates.** Sources are opened read-only, streamed into files created with `create_new`, hashed while copied, and flushed. A source that grows or changes size or modification time during the copy fails the attempt.
5. **Attempts are crash-recognizable.** Work happens in `<staging root>/stage_<32 hex>.partial` and is renamed to `stage_<32 hex>` only when complete. On ordinary failure or cancellation the partial directory is removed immediately. After a crash, `discard_abandoned_staging` removes only real directories matching that exact name pattern.

## Consequences

- Package paths are less human-readable than slug paths. Learners navigate by manifest titles, not paths, so this affects only people inspecting raw torrents.
- Re-attaching a file creates a new resource ID and therefore a new path. Content changes alter hashes regardless.
- Completed attempts accumulate until the publish workflow consumes or retires them; the retention policy belongs to release activation.
- The manifest bytes depend on `serde_json` pretty-printing. Upgrading it requires the determinism test to stay green, or a new ADR if the output changes.
