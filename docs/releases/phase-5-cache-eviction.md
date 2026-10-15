# Phase 5: free space taken by streamed lessons

Status: complete. Learners can remove streamed lessons they don't keep offline, one at a time or for a whole course, and can cap how much space streamed lessons use. Removal never touches kept lessons, `course.json`, or anything outside the course's own download folder. See [ADR 0008](../decisions/0008-learner-cache-removal.md).

## Delivered

- **`learning-application::CourseCache`:**
  - `remove(course, resources?)` stops the course's transfer, removes eligible files without following links, and re-adds the transfer with its selection and pause state. It reports the removed IDs, the freed verified bytes, and the kept IDs it refused.
  - `streamed_bytes`, `limit`, and `enforce_limit(skip_course)`, which removes least recently played lessons first.
  - The library gains `torrent_source`, `torrent_sources`, `playback_recency`, `cache_limit`, and `set_cache_limit`.
- **`learning-storage`:** `playback_recency(course)` returns the last-played time per resource.
- **Learner:**
  - Commands `remove_downloads { courseId, resourceIds? }`, `get_storage`, and `set_cache_limit { limitBytes }` (at least 256 MiB, or none). Error codes are `not_torrent_course`, `engine_failed`, `removal_failed`, and `library_query_failed`; none of them carries a path.
  - The cap is enforced after startup restore and after a lesson opens, sparing that course.
- **UI:**
  - "Remove download" appears on lessons that are partly or fully on the computer and not kept, except the one playing.
  - "Free up space" removes all of a course's non-kept lessons and reports what it freed.
  - The settings page has a "Storage" section with current use and a cap in GB.

## Acceptance evidence

- **End-to-end tests** `streamed_lessons_are_removed_and_the_transfer_goes_on`, `kept_lessons_are_never_removed_and_the_cap_spares_the_playing_course`, and `links_planted_in_a_course_folder_are_never_followed`, on loopback:
  - An unknown course is refused.
  - A streamed 3 MB lesson counts toward streamed bytes, and its removal reports the exact bytes.
  - Its disk blocks are freed even after librqbit re-creates the file. `course.json` stays, and the transfer comes back with the lesson unverified.
  - A kept lesson is refused by request and ignored by the cap.
  - The cap spares the skipped course and otherwise removes the lesson.
  - A link planted in place of the lesson file is left alone, and so is the file it points to.
- **Storage test:** playback recency.
- **Command test:** cap validation, and error mapping without paths.
- **Vitest:** the per-lesson remove button appears only for non-kept, downloaded lessons and sends that resource; "Free up space" sends the whole course and reports what it freed; the Storage form shows use, rejects caps under 0.25 GB, and saves in bytes.
- All repository gates pass.

## Current limitations

- Removing a lesson invalidates shared boundary pieces of neighbouring lessons (ADR 0008).
- The cap is checked at startup, when it changes, and when a lesson opens, not continuously while a lesson streams.
