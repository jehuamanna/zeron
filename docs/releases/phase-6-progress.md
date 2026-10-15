# Phase 6: lesson and course progress

Status: complete. The Learner tracks where the learner is with each lesson (not started, in progress, completed) and shows course progress in the library and in the course.

## Delivered

- **New crate `learning-domain`** (named in the system overview for progress and annotation rules; pure, with no dependencies):
  - `LessonStatus` and `CourseProgress`, with `of` and `percent`.
  - `watched_enough(position, duration)`: at least 90% of a video, or within its last 10 s.
- **`learning-storage` schema v8:**
  - `lesson_progress` holds `course_id`, `lesson_id`, `started_at`, and `completed_at`. It references `courses` only, so a re-import keeps progress.
  - `mark_lesson_started` is idempotent. `set_lesson_completed` keeps the first completion time; setting it to false clears it. `lesson_progress` reads it back.
  - Migrations after v4 now run from a table of steps, which keeps `migrate` readable.
- **`learning-application`:**
  - `record_resource_opened` starts the resource's lesson.
  - `save_playback_position` completes the lesson once the video is watched far enough.
  - `set_lesson_completed` validates the lesson against the manifest.
  - `course_progress` returns each lesson's status in manifest order, plus the summary.
- **Learner:**
  - `register_media` records the lesson as started. A failure there never blocks playback.
  - New commands `get_course_progress` and `set_lesson_completed`. Errors are `course_not_found` and `lesson_not_found`.
  - `list_courses` adds `completedLessons` and `totalLessons`.
- **UI:**
  - Library cards and the course header show a progress bar with "N of M lessons complete".
  - Each lesson shows a status icon (not started, in progress, completed) and a "Mark complete" toggle (`aria-pressed`).
  - Saved playback positions refresh progress, so watching to the end checks the lesson off.

## IPC and storage compatibility

- **IPC:** additive. There are two new commands, two new summary fields, and new types: `CourseProgress` and `LessonStatus`.
- **Storage:** migrating from v7 to v8 adds one table. Older builds refuse v8 with `FutureSchema`.

## Acceptance evidence

- **Domain tests:** the completion threshold, including non-finite values, statuses, and percent.
- **Storage test:** a start needs a known course and is idempotent; the first completion is kept; un-completing works; progress survives a re-import.
- **Application test:**
  - Lessons start not started.
  - Opening a resource makes its lesson in progress; an unknown resource is refused.
  - Watching 20% leaves it in progress; watching 93% completes it.
  - Completion persists across a library reopen. Manual un-complete and re-complete work, and an unknown lesson is refused.
- **Command test:** an unknown course is `course_not_found`.
- **Vitest:** library card progress bars; lesson status icons; "Mark complete" updates the bar from "1 of 2" to "2 of 2".
- All repository gates pass. A real-window check is part of the notes slice.

## Current limitations

- Progress is derived from video watching and manual marks. PDF reading does not count yet.
- One learner profile per installation.
