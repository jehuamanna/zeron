# ADR 0013: Zeron's own port mapper (PCP, NAT-PMP, UPnP; TCP and UDP)

Status: accepted (2026-10-16). Amends ADR 0004 decision 8.

## Context

Peers behind home routers can reach Zeron only if the router forwards its peer port. Since `443ca51`, engines accept uTP on UDP as well as TCP on the same port. librqbit 9.0.1 limits what its UPnP can do:

- `librqbit-upnp` `forward_port` hard-codes `TCP`, so uTP is never reachable from outside;
- it has no NAT-PMP or PCP, which many routers offer instead of, or as well as, UPnP;
- it reports results only in its logs.

Deluge (libtorrent) maps with UPnP and NAT-PMP by default. On 2026-10-16 the user asked for a Zeron port mapper covering UPnP for TCP and UDP, NAT-PMP, and PCP, and approved adding `netdev` to find the default gateway on every platform.

## Decision

1. **A crate with no torrent dependency.** `port-mapping` maps the ports it is given and knows nothing about torrents, so a future seed server can reuse it.
2. **Protocols, in order:** PCP (RFC 6887), then NAT-PMP (RFC 6886), then UPnP IGD. The first protocol that opens every port for both TCP and UDP holds the mappings.
   - A protocol that opens only some is undone before the next is tried.
   - A NAT-PMP-only router answering PCP with version 0 counts as "unsupported", per RFC 6887 §9.
3. **NAT-PMP and PCP** send MAP requests to the default gateway on UDP 5351.
   - **Retries:** a request is resent after 250 ms, then 500 ms, then 1 s, before counting as no answer.
   - **Replies:** only replies from the gateway's address are read. Each must match the request's opcode, transport and internal port, plus its nonce for PCP.
   - **Renewals and removal:** both reuse the PCP nonce. Lifetime 0 removes a mapping.
4. **UPnP IGD.**
   - **Discovery:** SSDP `M-SEARCH` for IGD:2 and IGD:1, waiting 2.5 s.
   - **Description:** the device description, preferring `WANIPConnection:2`, then `:1`, then `WANPPPConnection:1`.
   - **Mapping:** SOAP `AddPortMapping` and `DeletePortMapping` through `reqwest`, which is already compiled in.
   - **Untrusted answers:** a router's answers are untrusted. The description and control URLs must be plain HTTP on the IPv4 address that answered discovery, with no credentials. Redirects and proxies are not used, bodies are capped at 64 KiB, and requests time out after 5 s.
   - **Permanent leases:** a router that keeps only permanent mappings (error 725) gets a permanent mapping, which is removed at shutdown.
5. **Leases.** Each mapping asks for 2 h and is renewed at half the shortest lifetime granted. Permanent mappings are checked every 30 min. A failed renewal, or a changed external port, starts the protocol search again. When nothing worked, the mapper asks again after 5 min.
6. **Gateway.** `netdev::get_default_gateway` gives the IPv4 default gateway; only the `gateway` feature is enabled. The local address told to the router is the one the OS uses to reach the gateway, read from a connected UDP socket.
7. **Status.** The mapper publishes its state (`probing`, `mapped`, `unavailable`, `stopped`), the protocol holding the mappings, the open ports, and what each protocol answered (`mapped`, `no_answer`, `refused` with its code, `unsupported`, `no_gateway`, `failed`). The status carries no router text.
8. **Composition.**
   - The desktop app starts both engines with librqbit's UPnP off. When `PeerReachability::for_profile` keeps forwarding on (`Internet` and the user's choice), the app maps both engines' ports: the public engine's fixed port and the private engine's port.
   - The mapper's future runs on the Tauri runtime.
   - On exit, the app removes the mappings, bounded by 3 s, before the engines stop.
   - Network settings show the result, for example "Router: port 51413 open via NAT-PMP (TCP and UDP)." or "No router answered UPnP, NAT-PMP, or PCP. Forward TCP and UDP 51413 by hand for peers to reach you."

## Consequences

- uTP becomes reachable from outside when a router maps ports, and NAT-PMP or PCP routers now work.
- The user sees whether the router opened the ports, not only that Zeron asked.
- The cancellable, bounded mapper task renews and removes mappings, and a restart starts over. A crash leaves leased mappings to expire at the router; a permanent UPnP mapping stays until the next start's mapping replaces it, or until the router forgets it.
- IPv6 pinholes (PCP over IPv6) and hole punching are not covered. Carrier-grade NAT still blocks inbound connections.
- Any device on the LAN can make the same requests; that is why forwarding stays opt-in and scoped to `Internet`.
- Tests use fake PCP, NAT-PMP, and IGD servers on loopback; the real routers checked are listed in `docs/releases/phase-3-port-mapper.md`.
