# Phase 3: uTP alongside TCP

Status: complete. Engines listen on TCP and uTP on the same port and connect with both, as common torrent clients do.

## Delivered

- **`torrent-librqbit`:**
  - Every profile now uses librqbit's `ListenerMode::TcpAndUtp`. Outgoing connections try TCP first and add uTP after one second, or as soon as TCP fails; whichever connects first is used.
  - `PeerTransports` (`TcpAndUtp` by default, or `UtpOnly`) and `LibrqbitEngine::start_with_transports` exist for diagnostics and tests. `UtpOnly` also turns off outgoing TCP through librqbit's `ConnectionOptions::enable_tcp`.
  - `LibrqbitEngine::connection_counts` reports successful outgoing connections by transport.
- **Unchanged:** the port contract, and both apps, which use the default.

## Acceptance evidence

- **Engine test** `a_utp_only_learner_downloads_a_course_from_a_seeder` (loopback): a learner limited to uTP resolves the magnet from a seeder on TCP and uTP, then downloads all four files byte for byte. It made 0 TCP and at least 1 uTP connection. It passed 3 of 3 runs.
- The whole engine suite passes with uTP on. On this machine it ran in 17.6 s against 36 s before; TCP still wins on loopback.
- All Rust gates pass. No frontend files changed.

## Current limitations

- **Port mapping covers TCP only:** librqbit's UPnP maps TCP, so uTP is reachable from outside only through a manual UDP forward. The handover plans a Zeron port mapper (UPnP for TCP and UDP, NAT-PMP, PCP).
- **Not traversal on its own:** uTP alone does not get through NAT; that needs hole punching (BEP 55), which librqbit does not implement.
- librqbit's own comment still calls uTP not yet stable for default use. Watch for stalls in real swarms.
