# Phase 3: Admin seeding of published releases

Status: complete for starting, stopping, and restoring local seeding of immutable releases from the Admin app, with explicit network profiles.

## Delivered

- **Storage (publisher schema v3):**
  - `seeding_intents` records which releases the publisher wants seeded, with a foreign key to `releases`.
  - A `settings` table stores the network profile.
  - Migration from v2 runs in a transaction.
- **Network profile:** `PublisherDrafts::network_profile` and `set_network_profile` default to **Local network** when unset.
- **Seed request:** `PublisherDrafts::seed_request` structurally checks a release before seeding: exact package tree, manifest SHA-256 and torrent info hash against the record. The engine's piece check then verifies every byte before anything is served.
- **`SeedingService<E: TorrentEngine>`:**
  - `start` seeds a release and persists the intent, treating an already-active release as success. A failure leaves the intent unchanged and is remembered for display.
  - `stop` stops seeding and clears the intent without touching files.
  - `restore` restarts every intended release; individual failures don't block the others.
  - `statuses` returns every release with its intent, engine-local metrics, and failure. No storage lock is held across an engine await.
- **Admin backend:**
  - Startup launches `LibrqbitEngine` with the configured profile; state lives in app-data `engine/`. A failed engine start is non-fatal: authoring keeps working and the error is shown.
  - Intents are restored in the background, and the engine shuts down on app exit.
  - New commands: `get_distribution`, `start_seeding`, `stop_seeding`, `set_network_profile`. A profile change takes effect on the next launch.
- **Admin UI:**
  - The sidebar "Distribution" entry is now a workspace.
  - Network choices: Local network (default), Internet (public DHT, with a note that the IP address becomes visible to DHT users), and This computer only. A pending change shows "Restart Zeron Publisher to apply". The listening port is shown, and "UPnP port mapping is never used" is stated.
  - Each release has Start/Stop seeding and a state badge: Seeding, Checking N%, Not serving, Starting, or Not seeding.
  - Per-release metrics show peers, upload rate, and bytes uploaded this session, labelled as local to this computer and session, not download counts.
  - Fail-closed reasons and engine errors are displayed.

## Acceptance evidence

- **Storage test:** intents are idempotent, can be removed, and are ordered; unknown releases are rejected by the foreign key; the setting round-trips. The v1→v3 migration test still passes, now asserting schema 3.
- **Application tests (scripted engine):**
  - Start is idempotent, and the engine receives the release's package directory.
  - Statuses report intent and metrics.
  - A fresh service and engine restore exactly the intended release; stop is idempotent and clears the intent, after which restore starts nothing.
  - An engine refusal records the error without an intent and clears it on a later success.
  - An extra file in the release blocks seeding before the engine is called, and unknown releases are rejected.
  - The default profile is Local network.
- **Real-engine test (`torrent-librqbit`, loopback):** a published release reaches `Seeding` through `SeedingService`. After the engine shuts down, a new engine and service over the same database restore it to `Seeding` again; stop then clears both the intent and the transfer.
- **Admin command test:** the seeding status DTO falls back to the release size with no transfer, carries the last start error, maps the seeding state, peers and uploads, and parses network profiles.
- **Vitest:**
  - start/stop with local metrics and the port line;
  - checking percentage and the fail-closed reason;
  - saving the Internet profile with the restart notice and privacy text;
  - the engine-unavailable state disabling actions;
  - sidebar workspace switching.

  Two consecutive full runs passed.
- **Manual verification:**
  - The real debug binary, run with an isolated `XDG_DATA_HOME`, created schema 3 and an `engine/` state directory. `ss` showed exactly a TCP peer listener on an ephemeral port on all interfaces, UDP 6771 (local service discovery), and librqbit's idle UDP tracker socket. There was no DHT socket, matching the default Local-network profile.
  - In the browser pane with a fake IPC bridge, the Distribution workspace showed all three profiles. Selecting Internet showed the restart notice. Seeding, checking, and not-serving releases rendered with metrics, with no horizontal overflow at 1009px.
- All repository gates pass.

## Current limitations

- Seeding to real learners was verified with a librqbit learner on loopback, not on an actual LAN or the internet; the Local-network and Internet profiles need a manual multi-machine check.
- A network-profile change requires restarting the app; there is no live engine restart.
- The OS firewall may prompt when the Local-network or Internet profile listens on all interfaces.
- No bandwidth limits, fixed peer port, or tracker configuration yet.
- Seeding intent is restored per release only; metrics reset with every session.
