# Zeron

Zeron is an offline-first, torrent-powered course publishing and learning platform. The repository is being implemented as dependency-ordered vertical slices described in [the roadmap](docs/roadmap.md).

## Current implementation

The initial Rust workspace contains `course-format`, the framework-independent owner of the strict version-one `course.json` contract. It provides:

- Serde types with unknown-field rejection;
- semantic validation for stable IDs, ordering, SemVer, relations, hashes, media types, and HTTPS references;
- portable package-path validation, NFC enforcement, and case-collision detection;
- generated JSON Schema and a checked-in valid fixture.

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

Read [AGENTS.md](AGENTS.md) and the architecture documents before changing module boundaries or dependencies.

