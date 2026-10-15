# Phase 4: resume lessons where the learner stopped

Status: complete. This is Phase 4 item 2. The Learner remembers the playback position of each lesson resource and resumes there when the lesson is reopened, including after a restart. A finished lesson starts from the beginning.

## Delivered

- **`learning-storage` schema v5:**
  - The `playback_positions` table holds `course_id`, `resource_id`, `position_ms`, `duration_ms`, and `updated_at`. Its primary key is the course and resource.
  - The table references `courses` only, so a re-import, which rewrites `course_resources`, keeps positions.
  - Storage methods: `set_playback_position` (an upsert) and `playback_position`.
- **`learning-application`:**
  - `CourseLibrary::save_playback_position` rejects non-finite or negative values, durations over 48 h, and positions past the end, with the new `LibraryError::InvalidPlaybackTime`. It returns `false` for an unknown course or resource.
  - A position within the last 10 s is stored as 0. `playback_position` returns seconds.
- **Learner commands:**
  - `save_playback_position { courseId, resourceId, positionSeconds, durationSeconds } → boolean`. Errors are `invalid_input`, or `library_write_failed` with a fixed message, so storage errors never cross IPC.
  - `get_playback_position { courseId, resourceId } → number | null`.
- **UI:**
  - Opening a lesson fetches its position together with the media registration.
  - `LessonPlayer` resumes after `loadedmetadata`, but only from at least 5 s in and at least 10 s before the end. It shows "Resumed where you left off (m:ss)" with a "Start over" button.
  - It saves at most every 5 s while playing (never while seeking), and on pause, at the end, and when the player closes.
  - Data stays in the learner database, outside course packages.

## IPC and storage compatibility

- **IPC:** two new commands, which is additive.
- **Storage:** migrating from v4 to v5 creates one table. Older Learner builds refuse a v5 database with `FutureSchema`.

## Acceptance evidence

- **Storage tests:** an unknown course is rejected; upsert and read work; the position survives a re-import; overflow is rejected. A v4 database migrates and gains the table.
- **Application test:** positions persist across a library reopen; seven kinds of invalid input are rejected; unknown courses and resources are ignored; a position within the last 10 s resets to 0.
- **Command test:** invalid input maps to `invalid_input`, and an unknown lesson returns `false`.
- **Vitest:** resuming from a saved position and "Start over"; no resume from the first 5 s or the last 10 s; save throttling, and saves on pause, at the end, and on unmount; the App fetches the position with the lesson.
- **Real window**, with the user's "Systems Design Interview Questions" course:
  - Lesson 2 was seeked to 5:00, played to 5:06, and paused.
  - After restarting the Learner and reopening the lesson, playback resumed at 5:06–5:07 with "Resumed where you left off (5:06)".
  - "Start over" returned to 0:01 and kept playing.
- All repository gates pass.

## Current limitations

- Positions are not yet shown in the course outline. Course-level progress is Phase 6.
- One position is kept per resource, not per device or profile.
