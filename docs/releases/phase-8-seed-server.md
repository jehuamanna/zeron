# Phase 8: Headless seed server

Status: complete, except an image build in Docker (see limitations). `zeron-seed` keeps Zeron releases available from a machine peers can always reach. It follows a release's magnet, downloads it from the publisher's app (which dials out to it), and then serves learners even while the publisher is offline. It can also serve a release copied to it. Design: [ADR 0014](../decisions/0014-headless-seed-server.md). Operator guide: [seed-server.md](../operations/seed-server.md).

## Delivered

- **`tools/seed-server`** (binary `zeron-seed`), built from `torrent-port`, `torrent-librqbit`, and `port-mapping`. It runs one uploading engine on port 51413 under the `Internet` profile by default.
- **Seeding modes:**
  - `add <magnet> [--peer IP:PORT]…` follows a magnet: resolve, store the `.torrent`, download everything into `content/<info hash>/`, then serve it.
  - `add-release <release dir>`, or `--torrent F --content D`, serves a copied release read-only after full verification.
- **Seed list:** `seeds.json` in the data directory (by default `/var/lib/zeron-seed`), replaced atomically.
  - `add`, `add-release`, `remove`, and `list` edit it on the server; there is no remote management.
  - The running server follows changes within five seconds.
  - Magnets are accepted in hex or base32 `btih` form, up to 8 KiB; duplicates are refused.
- **Restore:** stored metadata and downloads resume on start without any peer. A task per torrent retries failures every minute, and metadata waits are capped at ten minutes per attempt.
- **Health:** `GET /health` on `127.0.0.1:8571` returns JSON per torrent: mode, state, problem, verified and total bytes, uploads, rate, and peers. It holds no paths. `zeron-seed health` exits 1 when the server does not answer.
- **Other options:** `--upload-limit` and `--download-limit` (KiB/s); `--map-ports` for servers behind home routers. SIGINT or SIGTERM stops the server cleanly.
- **Deployment:**
  - `deploy/seed-server/zeron-seed.service`: `DynamicUser`, `StateDirectory`, `ProtectSystem=strict`, no capabilities, `@system-service` syscall filter.
  - `deploy/seed-server/Dockerfile`: two stages, a non-root user, a volume, TCP and UDP 51413 exposed, and a health check. A root `.dockerignore` comes with it.

## Acceptance evidence

- **Hub test** (real engines on loopback):
  1. A publisher seeds a release, and the server follows its magnet until health shows `seeding` with every byte verified.
  2. The publisher shuts down, and a learner that can reach only the server downloads the 2.5 MB lesson byte for byte; the server counts the upload.
  3. The server restarts from its data directory with no peers, returns to `seeding`, and a second learner downloads the lesson again.
- **Release test:**
  1. A release added while the server runs is noticed and seeded. Adding it twice is refused.
  2. The HTTP `/health` answer reports `mode: release` and `state: seeding` and holds no paths.
  3. A learner downloads from the server. Removing the release stops serving it and leaves its files in place.
- **Unit test:** magnets yield the same hash in hex and base32; links without a v1 hash are refused.
- Three repeated runs of the hub tests were all green.
- **Real binary:**
  - `add-release` of this machine's published 2.4 GB release, then a duplicate `add` and a malformed magnet, both refused.
  - `serve --profile loopback`: `health` showed it `seeding` with 2,400,341,750 of 2,400,341,750 bytes verified.
  - `remove` while running emptied the list within seconds, and SIGTERM shut it down cleanly. `health` then exited 1, and the release files were untouched.
- **systemd:** `systemd-analyze verify` accepts the unit; it only notes that the binary is not installed on this machine.
- All repository gates pass.

## Current limitations

- **Docker:** the Docker image build was not run here, because this session has no access to the Docker socket. Run `docker build -f deploy/seed-server/Dockerfile -t zeron-seed .` once to confirm it.
- **Internet:** following a magnet across the internet was tested only on loopback; across the internet it needs the server's port open and the release's trackers or the DHT.
- **Remote transfer:** remote management and authenticated release upload are deferred to a security-reviewed step.
- **Disk:** no disk quota; the operator chooses what to follow.
- **Trust:** the server verifies every piece but does not check that a torrent is a Zeron course.
