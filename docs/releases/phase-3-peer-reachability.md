# Phase 3: peer port and router forwarding

Status: complete. Publishers and learners can choose a fixed peer port and opt in to UPnP router forwarding, so peers on other networks can reach a seeder. Like the network profile, the choice applies the next time Zeron starts.

## Delivered

- **`torrent-port::PeerReachability`:** a fixed port (1024–65535) or a new ephemeral port on each start, plus a UPnP choice. `for_profile` decides which parts apply:
  - **This computer only:** neither applies; the engine listens on an ephemeral loopback port.
  - **Local network:** the fixed port applies; forwarding does not.
  - **Internet:** both apply.

  Stored settings that are missing or invalid fall back to an ephemeral port without forwarding.
- **`torrent-librqbit`:** `EngineConfig::reachability` replaces `listen_port`. It sets the listen port and librqbit's `enable_upnp_port_forwarding`, which was previously hard-coded to off.
- **Admin:** `PublisherDrafts::peer_reachability` / `set_peer_reachability` persist the choice. The `set_peer_reachability` command rejects ports below 1024 as `invalid_input` with `field: "peerPort"`. `get_distribution` adds `peerPort`, `portForwarding`, `activePortForwarding`, and `restartRequired`, which now also covers a reachability change. The Distribution page has a peer-port form.
- **Learner:** the same settings through `CourseLibrary`, plus a `set_peer_reachability` command and a form in the Network view. Only the sharing engine takes the fixed port and forwarding. The private download engine always uses a new port, because it never serves peers. If sharing cannot start, the view says the fixed port may be in use and that downloads still work.

## Acceptance evidence

- **Unit tests:**
  - `PeerReachability` validation, profile scoping, and settings round-trip;
  - librqbit listener options per profile;
  - Admin command validation.
- **Persistence tests:** the Admin and Learner settings default to ephemeral without forwarding, persist across reopen, and can be reset.
- **Engine test** (`a_fixed_peer_port_is_bound_under_the_local_network_profile`, `--ignored` because it listens on every interface): the engine reports the chosen port and holds it. Passed locally.
- **Vitest:** port parsing, saving, validation errors, the restart notice, and the forwarding note, in both apps.
- **Real windows** (release builds, isolated app data, no course loaded, Internet profile):
  - **Admin:** after port 51413 and UPnP were saved, the page asked for a restart. After the restart it read "peers connect on port 51413. Your router is asked to forward this port with UPnP." The process listened on `*:51413` and opened one extra IPv4 UDP socket for UPnP discovery. After quitting, the port was closed.
  - **Learner:** port 51414 and UPnP were saved and persisted. After the restart, shared courses were served on 51414 and downloads used an ephemeral port. Only the sharing engine opened the UPnP discovery socket.
  - **Port in use:** with another process holding 51414, the Learner said "Sharing could not start, possibly because port 51414 is in use. Downloads still work." It no longer claims UPnP forwarding when sharing did not start; this was fixed after the check.
  - **Router:** the LAN router (192.168.1.1) sent no SSDP replies or announcements in 60 s, and no local firewall is enabled, so UPnP appears to be turned off on that router. No mapping was created. Accepting a real mapping is therefore unverified.
- All repository gates pass.

## Current limitations

- A change needs a restart; the engines are not restarted live.
- Whether the router accepted the UPnP mapping is not reported back. librqbit only logs it.
- Reaching a seeder from outside the local network was not tested. The verification router does not offer UPnP, and no outside host was available.
- Without UPnP, a manual router forward of the fixed port is needed. Carrier-grade NAT blocks inbound connections entirely; uTP and hole punching are not used.
- See ADR 0004 decision 8.

## Update, 2026-10-16: forwarding on by default, like common BitTorrent clients

- **Why:** a real two-machine test (Admin seeding under Internet, a Learner on another network) stayed at "finding metadata". The Admin's router answers none of UPnP, NAT-PMP, or PCP, and the Learner never mapped its own port, so neither side accepted connections.
- **Change:**
  - Forwarding is now on unless the user turns it off (`PeerReachability::from_settings`).
  - The Learner's private download engine also asks for a mapping on its ephemeral port. A seeder behind a router without port mapping can then dial the learner after finding it through the DHT, which librqbit re-queries at least every 60 s.
  - Forwarding still applies only under the Internet profile.
- **UI:** both apps say forwarding is on by default; the Learner explains that it lets seeders behind routers reach you. "Active now" reports forwarding when any engine runs.
- **Evidence:**
  - Updated unit and persistence tests for the new default and the explicit "off".
  - The Admin router probe (no UPnP, NAT-PMP, or PCP answers) and the public trackers listing the seeder on port 51413 are from the real test.
  - A connection through the learner's mapped port still needs confirming on the learner's network. See ADR 0004 decision 8.
