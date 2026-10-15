# Phase 5: disk-pressure handling

Status: complete. Zeron stops downloads before the disk fills up. The Learner refuses to keep lessons offline when they would not fit, and pauses running downloads when free space drops below 1 GB, resuming them once space returns. The Admin refuses to prepare a release whose staged copy would not fit.

## Delivered

- **Free space check:** `available_bytes(path)` uses `nix::sys::statvfs` (nix 0.30.1, `fs` feature; already in the lockfile and now a workspace dependency), on the path's nearest existing ancestor. The workspace forbids `unsafe` code, so this safe wrapper is used instead of calling `libc` directly. It is Unix only; elsewhere it returns `None` and no check refuses or pauses anything.
- **`learning-application`:**
  - `DISK_RESERVE_BYTES` is 1 GiB. `fits(available, needed)` checks that `needed` fits while leaving the reserve free.
  - `DiskGuard` pauses every running course the learner has not paused once free space falls below the reserve. It resumes only the courses it paused once free space exceeds 1.5 times the reserve; the gap prevents flapping.
  - The learner's own stored pause is never changed. Courses the learner paused are neither paused nor resumed by the guard.
- **Learner:**
  - A watchdog checks free space every 30 s once startup restore has finished.
  - `set_course_download` with `keep: true` computes the bytes still missing for every kept lesson. If they would not fit, it reverts the new commitment and returns `insufficient_space` with sizes in GB.
  - `get_storage` adds `availableBytes` and `lowDisk`.
  - The Storage section shows free space. Torrent courses show a banner while low disk space has paused downloads, and the player explains a stall the same way it does for a user pause.
- **Admin:** `PublishJobs::begin` sums the draft's source file sizes and calls `check_space` against the staging disk. The new error `DraftServiceError::InsufficientSpace` reports sizes in GB and maps to `insufficient_space`.

## Acceptance evidence

- **End-to-end test** `low_disk_space_pauses_downloads_and_recovery_resumes_only_those`, driving the guard with simulated free-space readings on a real loopback course:
  - Plenty of space leaves the course running; under the reserve it pauses.
  - The learner's stored choice stays unpaused, and the course stays paused within the hysteresis band.
  - Recovery resumes it.
  - A course the learner paused is never resumed by the guard.
  - On Unix, real `statvfs` readings are available.
- **Unit tests:** `fits`, and `check_space` with the exact rounded message ("about 0.5 GB … only 1.2 GB is free …"). The Learner's insufficient-space message reports gigabytes. The Admin maps the error to `insufficient_space`.
- **Vitest:** free space in the Storage section, and the low-disk banner in a torrent course.
- All repository gates pass.

## Current limitations

- Free space is not measured on Windows or other non-Unix platforms, so no disk-pressure handling happens there yet.
- The space estimate for keeping lessons offline ignores bytes in flight and filesystem overhead. The 1 GB reserve absorbs that.
- While downloads are paused for low disk space, lessons play only what is already verified on the computer.
