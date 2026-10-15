# Phase 5: keep lessons, modules, and courses offline

Status: complete. Learners can keep a lesson, a module, or a whole torrent course offline. Kept lessons download in full, the choice is re-applied after a restart, and each lesson shows how available it is without a network.

## Delivered

- **`learning-storage` schema v6:**
  - The `download_commitments` table holds `course_id`, `scope` (`course`, `module`, or `lesson`), `scope_id`, and `created_at`. It references `courses` only, so a re-import keeps commitments.
  - Storage methods: `set_download_commitment` (idempotent) and `download_commitments`.
- **`learning-application::downloads`:**
  - `DownloadScope` names a course, module, or lesson.
  - `CourseLibrary::set_download` refuses non-torrent courses and unknown module or lesson IDs.
  - `downloads` returns the stored scopes, and `committed_resources` the resources they cover.
  - `download_selection` returns the torrent indices of `course.json` plus every committed resource.
  - `Availability::of(kept, verified, length)` gives `online_only`, `downloading`, `partially_available`, or `available_offline`.
  - `TorrentImports::restore` now fetches each course with its committed selection, not just `course.json`, so kept lessons resume downloading after a restart.
- **Learner commands:**
  - `get_course_downloads` returns `{ course, modules, lessons }`.
  - `set_course_download { courseId, scope: { kind, id? }, keep }` saves the choice, then applies the new selection to the running transfer through `TransferRouter::select_files`. An inactive course picks it up on restore. Scope input is strict: unknown kinds and extra fields are rejected.
  - `get_course_availability` adds `kept` and `state` per resource.
  - Stopping a download never deletes verified bytes. Freeing space is the cache-policy slice.
- **UI:** torrent courses show "Keep the whole course offline", one toggle per module, and one per lesson, all with `aria-pressed`. A toggle covered by a larger scope shows as kept and locked. Each lesson has an availability label: Online only, Downloading for offline, Partly on this computer, or Available offline.

## IPC and storage compatibility

- **IPC:** two new commands and two new fields on resource availability, which is additive.
- **Storage:** migrating from v5 to v6 creates one table. Older Learner builds refuse v6 with `FutureSchema`.

## Acceptance evidence

- **Storage test:** a commitment needs a known course and a valid scope, is idempotent, can be removed, and survives a re-import.
- **End-to-end application test** `kept_lessons_download_in_full_and_are_selected_again_on_restore`, on loopback:
  - A magnet-imported course starts with only the manifest selected.
  - Unknown lessons, modules, and courses are refused.
  - Keeping the lesson adds its file to the selection; the label reads Downloading; selecting downloads it in full; and the label becomes Available offline.
  - A restore asks the engine for the same selection.
  - Module and course scopes resolve to the same file; releasing everything leaves only the manifest.
  - The labels for online-only and partially available lessons are covered.
- **Command test:** strict scope parsing, and a non-torrent course keeps nothing.
- **Vitest:** keeping a lesson sends the lesson scope and updates its toggle and label. Keeping the whole course locks the module and lesson toggles as kept, and the labels follow availability.
- **Real window**, with the user's "Systems Design Fundamentals" course seeded by the Admin:
  - Keeping `lesson12` moved it from "Online only" (0%) to "Available offline" (100%) within 3 s.
  - After restarting the Learner it was still kept and available offline.
- All repository gates pass.

## Current limitations

- A lesson next to a kept one can read "Partly on this computer" at 0%. It holds a piece shared at the file boundary, and the label reports that truthfully.
- There is no pause or resume, cache eviction, or disk-pressure handling yet; those are the next Phase 5 slices.
- Download progress refreshes with the 2 s availability poll.
