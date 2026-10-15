# Phase 3: Zeron port mapper

Status: complete. When the user lets Zeron open router ports (Internet profile, on by default), Zeron now does it itself. It uses PCP, NAT-PMP, or `UPnP`, opens both TCP and UDP (uTP), renews the mappings, and removes them when it quits. Network settings say what the router answered. Design: [ADR 0013](../decisions/0013-zeron-port-mapper.md), which amends ADR 0004 decision 8.

## Delivered

- **`port-mapping` crate** (no torrent dependency):
  - **Order:** PCP, then NAT-PMP, then `UPnP` IGD. The first protocol that opens every port for TCP and UDP holds the mappings; a partial success is undone before the next protocol.
  - **NAT-PMP and PCP:** MAP requests to the gateway on UDP 5351, with retries at 250 ms, 500 ms, and 1 s. Only replies from the gateway that match the request (and, for PCP, its nonce) are read. Renewals and removals reuse the PCP nonce.
  - **`UPnP` IGD:** SSDP discovery, the device description, and SOAP `AddPortMapping` and `DeletePortMapping`. Description and control URLs must be plain HTTP on the address that answered, with no redirects or proxies, 64 KiB bodies, and 5 s timeouts. Routers that keep only permanent mappings (error 725) get one, removed at shutdown.
  - **Leases:** 2 h each, renewed at half the shortest lifetime. A failed renewal searches again, and when nothing works, it asks again after 5 min.
  - **Status:** `probing`, `mapped`, `unavailable`, or `stopped`, plus the protocol, the open ports, and each protocol's answer.
  - **Gateway:** `netdev` (new workspace dependency, `=0.46.3`, `gateway` feature only) finds the default gateway on Linux, macOS, and Windows.
- **Desktop app:**
  - Engines start with librqbit's TCP-only `UPnP` off. When forwarding applies, the mapper opens the public engine's fixed port and the private engine's port.
  - On exit, the mappings are removed (within 3 s) before the engines stop.
  - `get_network_settings` gains `portMapping`.
- **Network settings:**
  - The option reads "Ask my router to open Zeron's ports (UPnP, NAT-PMP, PCP)".
  - The status line follows the router, for example "Router: port 51413 open via NAT-PMP (TCP and UDP)." or "No router answered UPnP, NAT-PMP, or PCP. Forward TCP and UDP 51413 by hand for peers to reach you."
  - The status refreshes every second while Zeron asks, then every 15 s.

## Acceptance evidence

- **Wire tests:** NAT-PMP and PCP requests encode as the RFCs lay out. Responses for another transport, port, or nonce are ignored, short packets are rejected, refusals carry their codes, and a NAT-PMP answer to PCP counts as unsupported.
- **`UPnP` tests:**
  - Discovery replies pointing at another host, HTTPS, a hostname, credentials, or `file:` are refused.
  - The WAN IP service is found in a nested description and resolved against its base; a control URL on another host is refused.
  - SOAP faults yield their `errorCode`.
- **Mapper tests** (fake gateways on loopback):
  - **PCP:** opens two ports for TCP and UDP (duplicates collapse), renews after half of a 2 s lifetime with the same nonces, and removes all four mappings at shutdown.
  - **NAT-PMP:** a NAT-PMP-only router is used after PCP reports "unsupported".
  - **Partial PCP:** PCP that refuses UDP has its TCP mapping undone before `UPnP` takes over.
  - **`UPnP`:** a router that refuses leases (725) maps permanently, and both mappings are deleted at shutdown.
  - **Silence:** when nothing answers, each protocol is reported as no answer.
  - Five repeated runs were all green.
- **DTO test:** the status crosses IPC as protocol and outcome names with refusal codes, without router text.
- **Vitest:** the router status sentence covers mapped (one or two ports, with a different external port), silence, refusals, probing, forwarding off, and other profiles. Network settings show it and follow a probing mapper until it maps.
- **Real router (this machine, gateway 192.168.1.1):** the router answers none of the three protocols, as the 2026-10-16 internet test found.
  - A standalone probe found the gateway through `netdev` and reported PCP, NAT-PMP, and `UPnP` each as no answer, then stopped.
  - In the real app (WebKitGTK, scratch data, Internet profile, port 51499), Network first showed "Asking your router to open port 51499…", then "No router answered UPnP, NAT-PMP, or PCP. Forward TCP and UDP 51499 by hand for peers to reach you."
- All repository gates pass.

## Current limitations

- No router that answers PCP, NAT-PMP, or `UPnP` was available, so a successful mapping is proven only against fake gateways. Check one on a router that offers port mapping, and record the protocol.
- IPv6 (PCP pinholes) and hole punching are not covered; carrier-grade NAT still blocks inbound connections.
- A crash leaves leased mappings to expire at the router; a permanent `UPnP` mapping stays until the router forgets it.
- The router is asked again only after a failed renewal or 5 min of no answer; a router that reboots and forgets the mappings is noticed at the next renewal.
