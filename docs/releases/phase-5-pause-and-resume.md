# Phase 5: pause and resume a course's transfer

Status: complete. A learner can pause all downloading and sharing for one torrent course and resume it later. Lessons already on the computer keep playing, and the choice survives a restart.

## Delivered

- **`torrent-port`:**
  - `TorrentEngine::pause` and `resume`. Both are idempotent, and by default they return `UnsupportedCapability`.
  - A new `TransferState::Paused`: a paused torrent downloads and serves nothing, but its verified bytes stay readable.
- **`torrent-librqbit`** (ADR 0004 decision 10):
  - The adapter uses `Session::pause` and `Session::unpause` (librqbit 9.0.1, `src/session.rs`) after a bounded wait for the initial check.
  - A seed paused because its content failed verification refuses `resume` and stays `Failed`.
  - Stats report `Paused` for a paused torrent that has no failure.
- **`learning-application`:**
  - `TransferRouter` delegates both calls and remembers paused torrents, so a course moved between engines by a sharing change is paused again.
  - `CourseLibrary::set_course_paused` and `course_paused` record the choice. `restore` re-pauses paused courses after re-adding them, with their kept-lesson selection.
- **`learning-storage` schema v7:** a `course_torrent_sources.paused` column (0 or 1, default 0). `TorrentCourseSource` gains `paused`.
- **Learner:**
  - The `set_course_paused { courseId, paused }` command applies the change to the running transfer. An inactive course picks it up on restore.
  - Errors are `not_torrent_course` and `pause_failed`. `get_course_downloads` adds `paused`, and course sharing status can report `paused`.
  - The Admin maps a paused release to `failed`, the only way a release is paused.
- **UI:**
  - Torrent courses have a "Pause downloads" / "Resume downloads" button (`aria-pressed`) and a note while paused.
  - While a course is paused, a buffering or stalled lesson explains that only the parts already on this computer play, instead of blaming peers.

## IPC and storage compatibility

- **IPC:** one new command and one new field, which is additive. The sharing state union gains `paused`.
- **Storage:** migrating from v6 to v7 adds one column. Older builds refuse v7 with `FutureSchema`.

## Acceptance evidence

- **Adapter test** `paused_transfers_download_nothing_until_resumed`:
  - Pausing an inactive torrent is `NotActive`, and pausing twice succeeds.
  - Selecting a lesson while paused downloads less than half of it in 2 s, and the state stays `Paused`.
  - `course.json` still streams while paused.
  - Resuming twice succeeds, and the lesson then completes.
- **Changed-content test:** a failed seed refuses `resume` and stays `Failed`.
- **Router test:** a pause follows the course to the sharing engine, and a resumed course stays running when it moves back.
- **End-to-end test** `paused_courses_are_restored_paused`: after the engine restarts, restore brings the course back in `Paused`.
- **Storage test:** the flag defaults off, can be set, and is refused for unknown courses.
- **Vitest:** the pause and resume button calls the command and shows the paused note; the player's paused-stall wording.
- All repository gates pass. A real-window check is part of the Phase 5 wrap-up.

## Current limitations

- Pausing is per course. There is no global pause yet.
- A lesson opened in a paused course waits for missing data until downloads resume.
