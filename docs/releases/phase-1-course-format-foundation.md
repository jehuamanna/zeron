# Phase 1 completion note: course-format foundation

Date: 2026-10-15

## Delivered scope

- Initialized the stable Rust 2024 workspace with exact direct-dependency versions and a committed lockfile.
- Added the framework-independent `course-format` crate.
- Implemented strict version-one manifest types and unknown-field rejection.
- Implemented semantic validation for schema version, stable IDs, SemVer, titles, language tags, finite durations, HTTPS URLs, MIME types, SHA-256 strings, contiguous ordering, resource relations, and thumbnail type.
- Implemented portable package-path policy: relative forward-slash paths, NFC normalization, control/traversal/drive-like segment rejection, Windows reserved-name rejection, reserved `course.json`, and case-folded collision detection.
- Added a valid fixture, generated JSON Schema, schema drift check, and negative validation tests.

## Acceptance evidence

Automated commands passed:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Five integration tests pass: valid round-trip, unknown-field rejection, aggregated semantic failures, path collision rejection, and checked-in schema parity.

## Manual verification

Run `cargo run -p course-format --bin export-course-schema /tmp/course.schema.json`, inspect the generated schema, then compare it with `crates/course-format/schemas/course-manifest-v1.schema.json`. Parse `tests/fixtures/valid-course.json` through a small caller or the test suite.

## Known limitations

- This slice validates manifest semantics only; package bytes, declared lengths, and hashes are not yet resolved against a filesystem or torrent inventory.
- Language validation is a conservative BCP-47-style syntax check, not the complete IANA language-tag registry.
- Unicode path collision checking uses lowercase folding rather than every target filesystem's exact normalization/case behavior; packaging must additionally verify its destination filesystem.
- The formal schema expresses structural constraints; semantic cross-reference and portability rules remain enforced by Rust validation.
- No Tauri, React, SQLite, torrent, publishing, or playback component is introduced in this slice.

## Next dependency-ordered slice

Implement a safe local package inventory/import use case around `course-format`: bounded manifest reading, symlink rejection, containment checks, exact file length and SHA-256 verification, and application-facing course summaries. This remains independent of Tauri and librqbit and prepares both local import and publisher validation.
