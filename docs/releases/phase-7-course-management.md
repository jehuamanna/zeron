# Phase 7 (pulled forward): course management view and release details

Status: complete. This is sub-slice 1 of the Admin course management work the user requested on 2026-10-16. The Courses page now groups drafts and releases by course. Each release opens to show its full magnet link with a Copy button, its info hash, size, declared files, status, and seeding state. "Export .torrent…" saves a copy of the published torrent through a native save dialog.

## Delivered

- **`PublisherDrafts::release_torrent`** (`publisher-application`): reads a recorded release's `release.torrent` with the existing size bound. It returns the bytes, the record, and the declared file table, but only if the torrent's info hash still matches the record.
  - Unknown, malformed, or path-like release ids, a missing torrent, and a torrent that no longer matches its record all fail with `InvalidAttempt`.
  - `ReleaseTorrent` and `PackageFile` are re-exported.
- **Admin commands:**
  - `get_release_details { releaseId }` returns `{ release: Release, files: [{ path, byteLength }] }`. File paths are the package-relative paths the torrent declares; filesystem paths never cross IPC. Failures use the code `release_unavailable`.
  - `export_release_torrent { releaseId }` loads and verifies the torrent, then opens a native save dialog in Rust. The suggested file name is portable, such as `rust-systems-1.2.0.torrent`, and the dialog has a `.torrent` filter. The command writes a fresh file with `fs::write`, so the exported copy is writable even though the release file is read-only. It returns `false` when the dialog is cancelled. A write failure returns `export_failed`.
- **Admin UI:**
  - `features/courses/CourseList.tsx` replaces the draft grid and the flat `ReleaseList`.
    - There is one card per `course_id`, ordered by latest activity. A card shows "Edit draft" buttons with each draft's version and revision, and the release history newest first.
    - Each release row shows its seeding state from `get_distribution`, polled every 5 s. If distribution is unavailable, the row says "Seeding state unavailable".
  - `ReleaseDetailsPanel.tsx` is the expandable details view (`aria-expanded`). It loads the file list lazily and has the "Export .torrent…" button.
  - `lib/clipboard.ts` copies with `navigator.clipboard.writeText`. If that is refused, it selects the read-only field and tries `execCommand("copy")`. If both fail, it leaves the link selected and asks the publisher to press Ctrl+C.

## IPC compatibility

The change is additive: two new commands and two new TypeScript types (`ReleaseDetails`, `ReleaseFile`). Existing DTOs are unchanged. There are no schema or migration changes.

## Acceptance evidence

- **`publisher-application` integration test** (`recorded_release_torrents_are_read_back_only_while_they_match_the_record`):
  - the exported bytes equal the on-disk torrent;
  - the file table matches the recorded file count and total size;
  - path-like and unknown ids are rejected;
  - a torrent modified on disk is refused.
- **Admin command tests:**
  - `release_details_and_torrent_exports_are_path_free`: the details DTO contains no scratch paths, the exported file is writable and byte-identical, and an invalid id maps to `release_unavailable`.
  - `suggested_torrent_names_are_portable`: covers traversal-like titles, non-ASCII titles, empty titles, and length bounding.
- **Vitest** (`CourseList.test.tsx`): grouping and ordering; per-course cards, drafts, and seeding labels; expanding to the full magnet, info hash, and files; Copy success and the manual-copy fallback; export cancel, success, and error; and degraded states.
- **Real window** (2026-10-16, Linux/Hyprland, WebKitGTK 2.52.6, `tauri-driver` 2.1.0 with WebKitWebDriver):
  - **Setup:** two releases (1.0.0, later superseded by 1.1.0) were published from a scratch fixture into an isolated `XDG_DATA_HOME` under the Loopback profile.
  - The single course card read "1 draft · 2 releases", and the statuses showed correctly.
  - Expanding 1.1.0 showed the exact magnet the fixture printed, the full info hash, and the two package files.
  - **Copy** used `navigator.clipboard` successfully. `wl-paste` returned the exact magnet, so the fallback is not needed on WebKitGTK.
  - **Export .torrent…** opened the native dialog and the command waited on it, with the button showing "Exporting…". Completing the dialog was not automated; see the limitations.
- All repository gates pass.

## Manual verification steps

1. Publish at least two versions of a course.
2. On Courses, check that both versions sit under one course card, and that the older one is marked Superseded.
3. Expand a release. Click Copy, then paste the magnet into the Learner's magnet import.
4. Click "Export .torrent…", save the file, and import it in the Learner. The saved file should be writable, and it should import as the same course version.

## Current limitations

- Saving through the export dialog was verified in Rust at the function level, not by driving the native GTK dialog. WebDriver cannot reach native dialogs.
- If the chosen file already exists, the export replaces it. Asking before overwriting is left to the native dialog.
- Seeding state on the Courses page updates every 5 s. Start and stop seeding stay on the Distribution page.
- Draft deletion, release archive, deleting release files, and drag and drop are the next sub-slices.
- Not checked on macOS or Windows.
