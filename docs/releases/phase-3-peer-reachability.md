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
- All repository gates pass.

## Current limitations

- A change needs a restart; the engines are not restarted live.
- Whether the router accepted the UPnP mapping is not reported back. librqbit only logs it.
- Reaching a seeder from outside the local network was not tested, because tests must not depend on public networks or a real router.
