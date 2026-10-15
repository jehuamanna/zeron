# One Zeron app

Status: in progress. Publisher and Learner are merging into one app, `apps/desktop`, following the user's decisions of 2026-10-16 in the handover. This note grows with each step and records evidence and limitations as they land.

## Delivered so far

### 1. The Learner becomes `apps/desktop`

The Learner app moved to `apps/desktop` (package `@zeron/desktop`, crate `zeron-desktop`). Its identifier stays `app.zeron.learner` until the data migration lands, so no intermediate build opens an empty library.

### 2. Publish runs inside it, on the public engine

- **Backend:** the Publisher's commands, Builder drops, and tracker-list fetching moved into a `publish` module. `PublishState` (drafts, publish jobs, releases, seeding, drop inbox) is managed beside the Library's `AppState`. Startup opens `publisher.sqlite3`, finishes interrupted release work, resumes seeding, and, under the Internet profile, refreshes stale tracker lists.
- **Two engines for the whole app:**
  - the **private** engine downloads courses and never uploads;
  - the **public** engine, on the one fixed port, serves shared courses and now also seeds releases (`SeedingService` runs on it).

  Each engine starts on its own, so releases can seed even if the private engine fails. Speed limits are one setting, applied to both engines.
- **One network setting:** the Library's profile, fixed port, `UPnP`, and speed limits govern the app. The Publisher's duplicate `set_network_profile`, `set_peer_reachability`, and `set_upload_limit` commands are gone, and `get_distribution` now reports only trackers, tracker lists, the release seeding statuses, and whether the public engine runs.

### 3. Your own release on both sides: seeding takes precedence

librqbit keeps one torrent per info hash in a session, so if Library shared a course that is also one of your releases, stopping it on either side would stop the other.

- Every release whose files are present is **reserved** on the public engine, at startup before Library transfers restore, and when a release is activated.
- A Library copy of a reserved torrent always stays on the private engine, even if it was stored as shared. Sharing it is refused with `own_release`.
- Importing your own release still works; the copy downloads privately and never uploads.
- Seeding a release that Library already shares from the same torrent is refused with `shared_in_library` until sharing stops there.

### 4. Library and Publish side by side

- **Shell:** a new `App` shows the Zeron header with **Library** and **Publish** always visible, plus one **Network** button. Network settings open over the current area, and Back returns to it.
- **Library:** the former Learner `App`, now `Library`. Its "Import local course" button sits beside the search.
- **Publish:** the Admin's Builder, courses, publish panel, and distribution screens, their API (`publish/services/publisher.ts`), store, and tests moved under `src/publish/`. `PublishArea` switches between **Courses** and **Distribution**.
- **Distribution:** keeps the trackers and the seeding list; its network profile, port, `UPnP`, and upload-limit forms are gone. A "Network settings" link opens the shared screen.
- **Network settings:** reworded for both sides. The port is "for sharing and seeding", the upload limit covers "sharing and seeding", and the status line says shared courses and releases are served on the public port.

## Acceptance evidence so far

- The Publisher's command tests run in the merged crate.
- **Router test:** a reserved torrent stored as shared still fetches into the private engine; sharing it is refused, and nothing reaches the sharing engine.
- **librqbit integration test:** the public engine seeds a release, the same release is imported by magnet from it into the private engine, and sharing the import is refused while the release keeps seeding.
- `own_release` maps to its own IPC code.
- **Vitest:**
  - The shell shows Library and Publish side by side, marks the current area, switches between them, and opens Network from the header or from Publish, returning to the area it came from.
  - The Publish area switches between Courses and Distribution.
  - Distribution offers no network controls, and its link opens Network settings.
  - All moved Admin suites and all Library suites pass: 22 files and 120 tests.
- **Browser preview** (Vite with faked IPC, 1180×760): Library, Publish (Courses and Distribution), and Network render in the shell; the Distribution link opens Network.
- All repository gates pass.

## Limitations so far

- The old Admin app still builds from `apps/admin-desktop` until the last step removes it.
- The Publisher's stored upload limit is not used; the Library's speed limits govern. The data migration will carry it over when the Library has none.
- A release reserved while Library already shares the same torrent in this session is not moved; it stays shared until restart, and seeding that release is refused meanwhile.
