# Zeron contributor guidance

Read `docs/architecture/system-overview.md`, `docs/architecture/distribution.md`, and `docs/roadmap.md` before changing architecture or dependencies.

## Non-negotiable boundaries

- Keep domain and application logic independent of Tauri, React, SQLite, HTTP, and librqbit.
- Keep course package bytes immutable after publication and learner data outside course directories.
- Do not expose librqbit types, filesystem paths, secrets, or database models over IPC.
- Never serve unverified torrent bytes or fabricate course structure before validating `course.json`.
- Do not add engine capabilities without a cited API and an integration test.
- Do not silently transcode, delete source media, remove release files, or overwrite annotations.
- New long-running work must be cancellable, bounded, observable, and recoverable after restart.
- Use controlled local torrent peers in CI; never depend on public trackers.

## Change discipline

Implement one roadmap phase/vertical slice at a time. Add crates only when they own working behavior. Every feature needs acceptance criteria, automated tests, manual verification steps, limitations, and a completion note. Record durable architectural changes as ADRs under `docs/decisions/`.

Rust uses stable toolchains, explicit error types, structured tracing with redaction, and workspace-pinned dependencies/lockfiles. TypeScript is strict; backend data uses TanStack Query and ephemeral UI state uses Zustand. Tauri handlers and React components remain thin.

For schema or IPC changes, update Rust types, generated/checked TypeScript contracts, JSON Schema where applicable, fixtures, migration/compatibility notes, and round-trip tests together.

## Security review triggers

Require focused review for path handling, Tauri capabilities/CSP, gateway changes, torrent engine upgrades, external processes, signing/key storage, remote management, deletion, migrations, and new telemetry. Treat manifests, torrent metadata, media, PDFs, archives, peers, trackers, and remote servers as untrusted.
