# Phase 2: authenticated loopback media gateway

Status: complete for verified, complete local files.

## Delivered

- `media-gateway` owns an HTTP server bound to an ephemeral IPv4 loopback port.
- A random per-process bearer secret and random opaque resource handles authorize requests.
- The learner registers media by stable course/resource IDs; filesystem paths never cross IPC.
- `GET` and `HEAD` support full responses and one strict byte range with correct `206` and `416` headers.
- Reads use 64 KiB chunks, eight concurrent streams by default, a 30-second body-frame timeout, and a registry capped at 256 resources.
- The desktop video player receives a capability URL only after an explicit learner selection.
- App exit revokes registered handles and requests graceful server shutdown.

## Acceptance evidence

- Unit coverage includes bounded range parsing, suffix/open-ended ranges, malformed and multi-range rejection, and constant-time capability comparison.
- Router-level coverage proves unauthorized rejection, partial body bytes, `HEAD`, and unsatisfiable-range behavior.
- Application recovery coverage proves media metadata and the backend-only package path survive restart.
- React coverage proves video selection requests an opaque URL and mounts the media player without a package path.

## Manual verification

1. Import a valid course folder containing a complete MP4 resource.
2. Open the course and select **Play video**.
3. Seek forward and backward; playback should issue byte-range requests and continue locally.
4. Exit Zeron; the loopback listener and all capability handles are revoked.

## Security review

- The listener is never bound to a LAN interface.
- Callers cannot submit a path to the gateway; registration begins with a manifest-backed application lookup.
- The gateway checks that the file is regular and its current length matches the verified manifest snapshot.
- Capability values and local paths are not logged.
- CSP allows media from IPv4 loopback only; other HTTP media origins remain blocked.

## Current limitations

- This slice serves only already-complete local files. Torrent-backed sparse readers, piece priorities, buffering state, and seek-aware scheduling remain Phase 3 work.
- The current reader abstraction is the registered file metadata boundary; Phase 3 will introduce an engine-neutral random-access source before librqbit is connected.
- Resource bytes are digest-verified during import, not re-hashed on every playback. A later same-length external modification is outside this slice's detection and should be handled by library integrity auditing.
- Multi-range requests are intentionally rejected rather than emitted as multipart responses.
