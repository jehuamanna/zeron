# One Zeron app

Status: in progress. Publisher and Learner are merging into one app, `apps/desktop`, following the user's decisions of 2026-10-16 in the handover. This note grows with each step and records evidence and limitations as they land.

## Delivered so far

### 1. The Learner becomes `apps/desktop`

The Learner app moved to `apps/desktop` (package `@zeron/desktop`, crate `zeron-desktop`). Its identifier stayed `app.zeron.learner` until the data migration landed, so no intermediate build opened an empty library.

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

### 5. Old data moves in once (ADR 0011)

- **Identity:** the identifier is now `app.zeron`, so data lives in `…/app.zeron`. The capability set is renamed `zeron-main`.
- **Import:** a new crate, `data-import`, runs first at startup. It imports `app.zeron.learner` and `app.zeron.publisher` from beside the new directory.
  - The databases are copied with `VACUUM INTO`, checked for integrity and schema version, and adjusted. Library locations under the old Learner directory are rebased (`Storage::rebase_locations`), and the Publisher's network choice and upload limit fill only what Library lacks.
  - A journal records the switch. `courses/`, `releases/`, and `staging/` move with one `rename` each, the databases switch in last, and only then are the old database files removed.
  - Every step resumes after a crash. A finished import, or a fresh install, does nothing again, and existing data in the new directory is never replaced.
  - If the import fails, startup stops, and the next start resumes.

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
- **Import tests (`data-import`):**
  - A fresh install records that there was nothing to import, and later old data is left alone.
  - With both directories: course files and the read-only release move with their permissions intact, the library points at the new course location, settings merge as decided, old databases are removed, engine state is untouched, and a second run changes nothing.
  - Library keeps its own network choice over the Publisher's.
  - Each directory imports on its own. Publisher-only data creates a library that carries its network choice.
  - **Crashes:** while copying, the copy starts again; part-way through the switch (blocked by a conflict), courses have moved but the databases are not yet switched, and the next run finishes; while cleaning, the cleanup finishes.
  - Existing data in the new directory is never replaced.
- **Storage test:** only locations under the moved directory are rebased, and a sibling directory whose name merely starts the same is not.
- **Real data:** a copy of this machine's `app.zeron.learner` (settings, an empty library, WebKit and engine state), imported in a scratch `XDG_DATA_HOME`. It became `app.zeron` with `courses/`, the library at schema v11, the Internet profile, port 51413 with `UPnP`, and a `done` journal; WebKit and engine directories stayed behind, and the second run reported `AlreadyDone`.
- **Browser preview** (Vite with faked IPC, 1180×760): Library, Publish (Courses and Distribution), and Network render in the shell; the Distribution link opens Network.
- All repository gates pass.

## Limitations so far

- The old Admin app still builds from `apps/admin-desktop` until the last step removes it.
- The old apps start empty after the import: their databases are removed. Running an old app while Zeron imports is unsupported.
- Webview storage (`localStorage` UI choices) is not carried over.
- A release reserved while Library already shares the same torrent in this session is not moved; it stays shared until restart, and seeding that release is refused meanwhile.
