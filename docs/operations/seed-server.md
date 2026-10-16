# Running a Zeron seed server

A seed server keeps your releases available from a machine peers can always reach, such as a small VPS with a public IP. It solves the case where your own router cannot accept connections (no `UPnP`, NAT-PMP, or PCP, and no port forward): your Zeron app dials out to the server and hands it the release, then learners download from the server even while your computer is off.

It runs headless on Linux and has no remote management. You edit its seed list on the server itself.

## How it gets a course

- **Follow a magnet** (`add`): the server joins the release's swarm, downloads it from whoever has it (usually your Zeron app), verifies every piece, and then keeps serving it. Your app only needs to be running and seeding the release until the server has a full copy. Health shows `downloading`, then `seeding`.
- **Serve a copied release** (`add-release`): copy a release directory from your machine, for example `~/.local/share/app.zeron/releases/release_<id>/`, with `rsync`, then point the server at it. The files are only read.

Both survive restarts: metadata and downloads stay in the data directory.

## Install with systemd

```bash
cargo build --release -p seed-server
sudo install -m 0755 target/release/zeron-seed /usr/local/bin/zeron-seed
sudo cp deploy/seed-server/zeron-seed.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now zeron-seed
```

Then open the peer port in the server's firewall: TCP and UDP 51413. For example, with ufw: `sudo ufw allow 51413/tcp` and `sudo ufw allow 51413/udp`.

Manage the list as root; the running server notices changes within five seconds:

```bash
sudo zeron-seed --data /var/lib/zeron-seed add 'magnet:?xt=urn:btih:…&tr=…'
sudo zeron-seed --data /var/lib/zeron-seed list
sudo zeron-seed --data /var/lib/zeron-seed remove <info-hash>
zeron-seed health
```

Copy the magnet from Publish → your course → release details. If the server cannot find your app through trackers or the DHT, give it your address: `add '<magnet>' --peer <your-public-ip>:<port>`. That works only if your side can accept connections; otherwise rely on trackers, because your app dials out to the server once both have announced.

For copied releases, grant the service read access to their directory with `ReadOnlyPaths=` in a drop-in (`systemctl edit zeron-seed`).

## Run with Docker

```bash
docker build -f deploy/seed-server/Dockerfile -t zeron-seed .
docker run -d --name zeron-seed --restart unless-stopped --network host \
  -v zeron-seed:/var/lib/zeron-seed zeron-seed
docker exec zeron-seed zeron-seed add 'magnet:?xt=urn:btih:…'
docker exec zeron-seed zeron-seed health
```

Host networking keeps the advertised port and the real port the same. With port publishing instead, publish both transports: `-p 51413:51413/tcp -p 51413:51413/udp`.

## Options

`zeron-seed serve`:

| Option | Default | Meaning |
|---|---|---|
| `--port N` / `--any-port` | 51413 | Peer port (TCP and UDP) |
| `--profile` | `internet` | `internet` uses the DHT and the release's trackers; `local_network` only finds peers on the LAN |
| `--upload-limit` / `--download-limit` | none | KiB/s, at least 16 |
| `--map-ports` | off | Ask a home router to open the port (PCP, NAT-PMP, `UPnP`) |
| `--health ADDR` / `--no-health` | `127.0.0.1:8571` | Read-only JSON status at `/health` |

`--data DIR` (or `ZERON_SEED_DATA`) sets the data directory, by default `/var/lib/zeron-seed`.

## Health

`GET /health` returns:

- the peer port;
- the router status, when `--map-ports` is set;
- for each torrent: its mode, its state (`finding_metadata`, `downloading`, `checking`, `seeding`, `retrying`, `failed`), the problem if any, verified and total bytes, bytes uploaded this session, the upload rate, and connected peers.

Upload counts are this server's own and are not download counts. Keep the endpoint on loopback unless a monitor needs it; it changes nothing but shows which torrents you serve.

## Limits

- Anyone with a release's magnet can download it from the server. Do not follow private courses.
- The server checks every piece but not that a torrent is a Zeron course; it serves what you list.
- There is no disk quota; followed torrents download in full into the data directory.
- Removing a torrent keeps its files. Delete `content/<info-hash>` yourself if you want the space back.
