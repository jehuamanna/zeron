# ADR 0012: One Zeron desktop app

Status: accepted (2026-10-16). Supersedes the "two Tauri apps in one workspace" desktop topology in `system-overview.md`.

## Context

Zeron shipped as two separately packaged Tauri apps: Learner (`app.zeron.learner`) and Publisher (`app.zeron.publisher`). Each ran its own torrent engines, network settings, and data directory. A real internet test on 2026-10-16 showed what that costs. Someone who both publishes and learns had to configure two network profiles, two fixed ports, and two `UPnP` forwards, with two engines competing for the router. The user decided on 2026-10-16 to merge them into one app.

## Decision

1. **One app.**
   - The product is named "Zeron", with identifier `app.zeron`. It is built from `apps/desktop`, package `@zeron/desktop`, crate `zeron-desktop`.
   - Linux ships one AppImage, which keeps `bundleMediaFramework: true` for in-app playback.
   - `apps/admin-desktop` and `apps/learner-desktop` are gone.
2. **Navigation.** Top-level **Library** (learning) and **Publish** (courses, Builder, distribution) sit side by side and are always visible, with one **Network** screen for both. Publish code lives under `apps/desktop/src/publish/` and keeps its own API module and store; Library code stays where the Learner had it.
3. **One network setting, two engines.**
   - The profile, fixed port, `UPnP` choice, and speed limits are stored in the library database and govern the whole app.
   - A **private** engine downloads courses and never uploads, on a new port each start.
   - A **public** engine, with uploads on and the one fixed port, serves the courses the learner shares and seeds published releases. `SeedingService` runs on it.
   - Each engine starts on its own, so seeding works if the private engine fails, and the reverse. Speed limits apply to both.
   - Tracker settings stay with the Publisher's data.
4. **Your own release on both sides.** librqbit keeps one torrent per info hash in a session, so a release and a shared Library copy of it would share one transfer, and stopping either would stop both. Seeding takes precedence:
   - Every release with files present is reserved on the public engine, before Library restores transfers and when a release is activated. `TransferRouter::reserve` keeps a reserved torrent private.
   - Sharing a Library copy of one is refused with `own_release`.
   - Seeding a release that Library already shares is refused with `shared_in_library`.
   - Importing your own release still works, as a private copy that never uploads.
5. **Composition.** One composition root, `apps/desktop/src-tauri/src/lib.rs`, does all of this in order:
   1. imports old data (ADR 0011);
   2. opens the library;
   3. starts the HTTPS media gateway on Linux;
   4. starts both engines;
   5. opens Publish, which reserves releases;
   6. loads sharing choices;
   7. restores transfers and seeding;
   8. refreshes tracker lists under the Internet profile.

   The Library's `AppState` and Publish's `PublishState` are separate managed states. Shutdown stops the gateway and both engines.

## Consequences

- One port to open and one `UPnP` mapping cover sharing and seeding.
- Publish and Library each keep their IPC error shapes, `{ code, message }` and `{ code, message, field?, currentRevision? }`. The Publisher's duplicate network commands are removed.
- The Distribution page no longer edits network settings; it links to Network.
- Old data moves in once on first start (ADR 0011). The old apps then start empty.
- A future headless seed server (Phase 8) still reuses `torrent-port`, `torrent-librqbit`, and `publisher-application`; nothing here depends on the desktop shell.
