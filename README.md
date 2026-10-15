# Zeron

Zeron is an offline-first, torrent-powered course publishing and learning platform. The repository is being implemented as dependency-ordered vertical slices described in [the roadmap](docs/roadmap.md).

## Current implementation

The initial Rust workspace contains `course-format`, the framework-independent owner of the strict version-one `course.json` contract; `learning-application`, which owns safe local-folder import; and `learning-storage`, which owns local persistence. It provides:

- Serde types with unknown-field rejection;
- semantic validation for stable IDs, ordering, SemVer, relations, hashes, media types, and HTTPS references;
- portable package-path validation, NFC enforcement, and case-collision detection;
- generated JSON Schema and a checked-in valid fixture.
- bounded and cancellable local import with symlink rejection, containment checks, and exact length/SHA-256 verification.
- bundled SQLite migrations, foreign-key enforcement, WAL negotiation, FTS5 capability checks, transactional course snapshots, and local metadata search.
- backend-only package-location recovery and a path-free course/module/lesson/resource navigation interface.
- authenticated loopback playback for complete verified video resources, including strict byte-range responses and bounded streaming.
- a publisher-domain foundation with revision-safe course drafts, structural validation, and explicit immutable-release workflow transitions.
- durable publisher autosave snapshots with restart recovery and stale-editor protection.

The latest completed slice is documented in [Phase 2: authenticated loopback media gateway](docs/releases/phase-2-loopback-media-gateway.md).

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Regenerate the manifest schema after changing its Rust types:

```bash
cargo run -p course-format --bin export-course-schema
```

Exercise the current local-library flow without a desktop shell:

```bash
cargo run -p course-cli -- ./zeron.sqlite3 import /path/to/course
cargo run -p course-cli -- ./zeron.sqlite3 list
cargo run -p course-cli -- ./zeron.sqlite3 search recovery
```

Run the Learner Desktop during development:

```bash
pnpm install --frozen-lockfile
pnpm --filter @zeron/learner-desktop tauri dev
```

Read [AGENTS.md](AGENTS.md) and the architecture documents before changing module boundaries or dependencies.
