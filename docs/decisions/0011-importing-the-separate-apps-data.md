# ADR 0011: Importing the separate apps' data into the one Zeron app

Status: accepted (2026-10-16)

## Context

The user decided on 2026-10-16 to merge Learner (`app.zeron.learner`) and Publisher (`app.zeron.publisher`) into one app with identifier `app.zeron`, importing the old data once on first start. Each app keeps its data in its own app-data directory, and the platform places those directories side by side under one parent:

- **Learner:** `library.sqlite3` and the torrent downloads in `courses/`. The library stores each torrent course's absolute download directory, such as `…/app.zeron.learner/courses/<id>`.
- **Publisher:** `publisher.sqlite3`, the read-only release directories in `releases/`, and `staging/`.
- **Both:** `engine*` directories that hold no persistent state.

Course downloads and releases can be many gigabytes. The move touches the user's only copy of their library and releases, and a crash or power loss can interrupt it at any point. Path handling and migrations are security-review triggers in AGENTS.md.

## Decision

1. **Where it lives.** A crate, `data-import`, owns the import. `apps/desktop` calls `import_old_data(ImportPlan::beside(app_data))` first in startup, before anything opens a database. The old directories are the target's siblings named `app.zeron.learner` and `app.zeron.publisher`.
2. **What moves.**
   - The databases are copied: they are small, and the library copy needs its stored paths rewritten.
   - `courses/`, `releases/`, and `staging/` move with one `rename` each: no copy and no extra disk space. Moving the `releases` directory as a whole keeps each read-only release directory and its permissions as they were.
   - `engine*` directories are neither moved nor removed.
3. **Copy, verify, adjust, switch, then remove.**
   1. **Stage.** Copy each old database with `VACUUM INTO` into `<target>/.data-import/`. This makes a consistent snapshot that includes anything still in the write-ahead log. Then check that the copy passes `PRAGMA integrity_check` and has the same `user_version` as the original.
   2. **Adjust the copies.**
      - Library locations under the old Learner directory are rebased onto the target (`Storage::rebase_locations`). Locations elsewhere, such as local folder imports and draft sources, are unchanged.
      - The Publisher's `network_profile`, `peer_port`, and `port_forwarding` are copied into the library only when the library has none of the three. Its `upload_limit_kib` fills only a missing library upload limit. With no old library but such settings, a new library is staged to carry them.
      - Each staged database is then sealed: journal mode `DELETE`, integrity checked again, and no side files.
   3. **Journal `switching`.** `data-import.json` is written atomically: a temporary file, `fsync`, rename, then a directory `fsync`.
   4. **Switch.** Move the directories, then rename the staged databases into place. The databases go last, so the app can never open a library whose courses have not arrived. A destination that already holds something is never replaced: an empty directory is, but anything else stops the import with `Conflict`.
   5. **Journal `cleaning`.** Remove the staging directory and the old database files (`*.sqlite3`, `-wal`, `-shm`, `-journal`).
   6. **Journal `done`.**
4. **Idempotence and recovery.**
   - With no journal, any leftover staging directory is our own interrupted copy. It is removed and the copy starts again; the originals were never touched.
   - With journal `switching` or `cleaning`, every step checks whether it is already done, so a crash anywhere converges.
   - With journal `done`, nothing happens, even if old data reappears later.
   - A target that already has a database never receives an imported one. A fresh install writes `done` with nothing imported.
5. **Failure stops startup.** If the import fails, Zeron does not start: opening empty databases over a half-finished switch could later block or confuse the resumed import. The error goes to the process log, and the next start resumes.
6. **No paths over IPC.** The import runs before the window has any state, and its errors stay in the process log.

## Consequences

- Library courses, notes, drawings, progress, sharing choices, drafts, releases, seeding intents, and tracker settings appear in the one app after its first start, without copying course media.
- The old apps, if still installed, start empty: their databases are gone. Running an old app while Zeron imports is unsupported, because the rename would pull files out from under it.
- The import needs the old and new directories on one filesystem, which sibling app-data directories are. A cross-device `rename` fails with `Io`, nothing is lost, and startup stops until the cause is fixed.
- Webview storage, such as `localStorage` remembered UI choices, is not carried over.
- Tests cover a fresh install, both directories, each directory alone, Library keeping its own network choice, crashes while copying, switching, and cleaning, a second run that does nothing, and existing target data that is never replaced. The read-only release directory keeps its permissions in those tests.
