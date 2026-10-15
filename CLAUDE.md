# Zeron — Claude Code guidance

Read [AGENTS.md](AGENTS.md) first. Its architecture, security, and change-discipline rules are authoritative for every task.

When resuming the current implementation, read [docs/handover/CLAUDE-CODE-HANDOVER.md](docs/handover/CLAUDE-CODE-HANDOVER.md), then inspect `git status`, `git diff`, and the latest commits before editing. The handover distinguishes committed work, user-owned changes, and the active incomplete slice.

For architectural or dependency work, also read:

- [docs/architecture/system-overview.md](docs/architecture/system-overview.md)
- [docs/architecture/distribution.md](docs/architecture/distribution.md)
- [docs/architecture/admin-publisher.md](docs/architecture/admin-publisher.md)
- [docs/roadmap.md](docs/roadmap.md)

## Delivery loop

Implement one dependency-ordered vertical slice at a time. A slice is complete only when its acceptance behavior, automated tests, manual verification notes, limitations, and release note under `docs/releases/` agree.

Before each commit, run the relevant narrow tests and then the repository gates:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
pnpm test
pnpm typecheck
pnpm build
git diff --check
```

Commit and push each green slice to `main` with a focused conventional commit. Preserve unrelated working-tree changes. In particular, the current `AGENTS.md` modification is user-owned and must stay out of implementation commits unless the user explicitly asks to commit it.

Use `apply_patch` for source and documentation edits. Keep filesystem paths, secrets, database models, and engine types behind Rust boundaries. Tauri commands translate path-free DTOs; React components remain thin.
