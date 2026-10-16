# ADR 0014: Headless seed server

Status: accepted (2026-10-16)

## Context

Two peers whose routers cannot accept connections cannot reach each other over the internet. This machine's router answers no port-mapping protocol (ADR 0013), and the user's second machine did not connect. On 2026-10-16 the user asked for Phase 8, the headless seed server, so that one always-reachable machine can serve releases for everyone. Remote management is a security-review trigger in AGENTS.md and stays out of this step.

## Decision

1. **A Linux-first binary**, `zeron-seed`, in `tools/seed-server`.
   - It is composed from `torrent-port`, `torrent-librqbit`, and `port-mapping`, with no Tauri, desktop, or learner code.
   - It runs one engine with uploads on, on a fixed port (51413 by default), under the `Internet` profile by default.
2. **Two ways to seed:**
   - **Follow a magnet.** The server resolves the magnet's metadata through the DHT, trackers, and optional `--peer` addresses. It stores the `.torrent`, downloads every file into `content/<info hash>/`, and keeps serving it. A publisher whose router is closed reaches the server by dialling out, so the server becomes the reachable hub.
   - **Serve a copied release** read-only, through the adapter's existing read-only seeding storage, after full verification.
3. **The seed list is a file, not an API.**
   - `seeds.json` lives in the data directory and is replaced atomically.
   - Management commands (`add`, `add-release`, `remove`, `list`) edit it on the server, under the operator's file permissions.
   - The running server checks the file's modification time every five seconds and follows the changes. No network interface can change what is served.
4. **Restore.**
   - Stored metadata means a restart needs no peer: followed torrents re-check their downloaded files, and releases re-verify theirs.
   - Each torrent has a task that retries failures every minute. Metadata waits time out after ten minutes and are then retried.
5. **Health.**
   - The endpoint is a read-only `GET /health` JSON document on `127.0.0.1:8571` by default. It shows each torrent's state, problem, verified bytes, upload counters, and peers, with no filesystem paths.
   - `zeron-seed health` queries it and exits non-zero when the server does not answer, for Docker `HEALTHCHECK` and monitors.
6. **Bandwidth.** Session-wide upload and download caps (`--upload-limit`, `--download-limit`, at least 16 KiB/s) use the adapter's rate limits.
7. **Deployment.**
   - **systemd:** a hardened unit with `DynamicUser`, `StateDirectory`, `ProtectSystem=strict`, an empty capability set, and the `@system-service` syscall filter.
   - **Docker:** a two-stage image with a non-root user, a volume, both transports exposed, and a health check.
8. **Router ports.** A server behind a home router can pass `--map-ports` to use the same mapper as the desktop app.

## Consequences

- One reachable server makes releases available to learners anywhere, even while the publisher is offline. Publishers whose routers cannot be opened have a way out.
- Anyone with the magnet can download from the server, as from any seeder. Private courses remain out of scope.
- **Deferred to a later, security-reviewed step:** authenticated, staged remote transfer of releases and remote management.
- No disk quota is enforced; the operator chooses what to follow.
