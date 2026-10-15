# Phase 1 completion note: safe local import

Date: 2026-10-15

## Delivered scope

- Added `learning-application` as a framework-independent application-use-case crate.
- Added configurable manifest size, resource count, and aggregate resource byte limits.
- Added cooperative cancellation before work, between files, and between hash chunks.
- Added local-root and regular-manifest checks with manifest validation before course structure is returned.
- Added path component, root containment, and per-component symlink rejection.
- Added regular-file, declared-length, and SHA-256 verification for every manifest resource.
- Added a metadata recheck to detect source files changed during hashing.
- Returned only stable resource IDs and package-relative paths; absolute OS paths remain backend-local.

## Acceptance evidence

Formatting, strict Clippy, and all workspace tests pass. The local import integration suite covers a complete package, length mismatch, digest mismatch, configured limits, preexisting cancellation, and symlinked path components. Together with `course-format`, the workspace currently has eleven passing integration tests.

## Manual verification

Create a directory with a valid `course.json` and matching resource bytes, invoke `import_local_course` from a small Rust caller, and confirm that its `ImportedCourse` contains the validated tree and relative-path verification evidence. Change a resource byte and confirm import fails without returning a course.

## Security review

The manifest is treated as untrusted before paths are joined. `course-format` admits only normalized relative paths, and local import independently rejects non-normal platform components. Every existing path component is checked with `symlink_metadata`, the result is canonicalized and checked under the canonical root, and only regular files are hashed. Error display avoids absolute path disclosure.

The standard filesystem APIs cannot make the check/open sequence fully race-free against an adversary concurrently replacing path components. The publisher's immutable staging implementation must use capability-relative or platform `openat`/no-follow semantics before this code is accepted for hostile multi-user directories. The current control is appropriate for user-selected, locally owned course directories, with source mutation detection after opening.

## Known limitations

- The use case is synchronous and must run on a bounded blocking executor in an async desktop composition root.
- Cancellation latency is bounded by one 1 MiB file read plus storage latency.
- Extra undeclared files are ignored; publisher validation will report them under packaging policy.
- No persistence, Tauri command, or UI has been added yet.

## Next dependency-ordered slice

Add `learning-storage` with bundled SQLite, transactional migrations, startup FTS5 capability verification, and repositories for imported courses and readiness/recovery state. Keep repository models private and expose application-owned records.
