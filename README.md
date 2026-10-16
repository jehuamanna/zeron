# Zeron

Zeron is an offline-first, torrent-powered course publishing and learning platform. The repository is being implemented as dependency-ordered vertical slices described in [the roadmap](docs/roadmap.md).

## What Zeron does

- **Library:** import courses from folders, magnet links, `.torrent` files, or signed catalogs. Stream lessons while they download, playing only verified bytes. Take notes anchored to video moments and PDF pages, draw, track progress, and search offline.
- **Publish:** build courses by dropping folders, publish immutable versioned releases with reproducible torrents, seed them, sign them with a keychain-held identity, and list them in a signed catalog.
- **Trust:** the Library pins each course to its first signed publisher and refuses updates from anyone else (ADR 0014).
- **Platforms:** Linux, macOS, and Windows, all built and tested in CI ([platform matrix](docs/testing/platform-matrix.md)).

Start with the [user guide](docs/user-guide.md). Maintainers release with [docs/operations/releasing.md](docs/operations/releasing.md). Each completed slice is documented under [docs/releases](docs/releases/), and the roadmap is in [docs/roadmap.md](docs/roadmap.md). Zeron is one desktop app with Library and Publish side by side ([ADR 0012](docs/decisions/0012-single-desktop-app.md)).

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
pnpm test
pnpm typecheck
pnpm build
```

Audits and deeper checks:

```bash
cargo deny check
pnpm licenses:check
ZERON_KEYCHAIN_TESTS=1 cargo test -p publisher-identity --test os_store
cd fuzz && cargo +nightly fuzz run manifest
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

Run the Zeron desktop app (Library and Publish) during development:

```bash
pnpm install --frozen-lockfile
pnpm --filter @zeron/desktop tauri dev
```

Read [AGENTS.md](AGENTS.md) and the architecture documents before changing module boundaries or dependencies.
