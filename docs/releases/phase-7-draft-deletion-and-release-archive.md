# Phase 7 (pulled forward): draft deletion and release archive

Status: complete. This is sub-slice 2 of the Admin course management work. Publishers can delete a draft and archive a release from the Courses page. Archiving stops seeding and keeps the release files.

## Delivered

- **`publisher-storage`** (no schema change; `archived` was already an allowed status):
  - `delete_draft(draft_id, expected_revision)` uses the same compare-and-swap rule as autosave. It returns `false` for an unknown draft and a `RevisionConflict` with the current revision when the draft was saved since. Releases keep their `draft_id` as history.
  - `archive_release(release_id)` marks the release `archived` and removes its seeding intent in one transaction. It is idempotent, and other releases keep their status. An archived version still counts as released, so the version number can never be reused.
- **`publisher-application`:**
  - `PublisherDrafts::delete_draft` (`NotFound` for unknown drafts) and `PublisherDrafts::archive_release`.
  - `SeedingService::archive` stops the transfer first and archives only if the engine stopped it. If the engine refuses, the release stays published.
  - `seed_request`, and therefore `start` and `restore`, refuse archived releases with the new `DraftServiceError::ReleaseArchived`.
  - `PublishJobs::draft_in_progress(draft_id)` reports a job that is running or `READY_TO_PUBLISH`. The delete command refuses such drafts with the new `DraftServiceError::DraftBusy`.
- **Admin commands:**
  - `delete_draft { draftId, expectedRevision }`. Errors: `draft_busy`, `revision_conflict` (with `currentRevision`), `draft_not_found`.
  - `archive_release { releaseId } → Release`. It uses the seeding service when the engine is running, and otherwise archives the record directly, since nothing is seeding in that case. Errors: `release_archive_failed`, `seeding_failed`. `release_archived` is the new code for a refused seed start.
- **Admin UI:**
  - `ConfirmAction` is an inline `alertdialog`. Focus starts on Cancel.
  - Each draft has a delete button. The confirmation says that releases and original media are kept. Busy-draft and conflict errors are explained, and a conflict refetches the list.
  - Release details have an "Archive release…" button whose confirmation explains the consequences. Archived releases show a note in place of the button.
  - The Distribution page disables "Start seeding" for archived releases, with a tooltip.

## IPC compatibility

The change is additive: two commands and two error codes. DTOs are unchanged.

## Acceptance evidence

- **Storage tests:**
  - deleting with a stale revision is a conflict, deleting an unknown draft returns `false`, and deleting a draft never removes releases;
  - archiving drops only that release's intent, is idempotent, returns `None` for an unknown release, and still blocks reuse of the version.
- **Application tests:**
  - `archiving_stops_seeding_keeps_files_and_refuses_restarts` runs against a scripted engine. It covers an engine refusal that leaves the release published, a successful archive that keeps the other release seeding, read-only files left in place, `start` and `seed_request` refusing with `ReleaseArchived`, idempotence, and `restore` after a restart starting only the remaining intent.
  - Draft deletion with a stale revision and with an unknown draft.
  - `draft_in_progress` for running and ready jobs, and not for failed or cancelled ones.
- **Command tests:** the codes `draft_busy`, `revision_conflict` with `currentRevision`, and `draft_not_found`; archiving without an engine; `release_archived` for a refused seed request.
- **Vitest:** the delete confirmation and cancel, deletion that keeps the course's releases, the busy and conflict messages, archive confirmation and its archived result, a failed archive, and the disabled Distribution button.
- **Real window** (WebKitGTK through `tauri-driver`, isolated `XDG_DATA_HOME`, Loopback profile):
  - Started seeding 1.1.0, then archived it from Courses. The row changed to "Not seeding · Archived", and the Distribution start button was disabled with its tooltip.
  - Deleting the draft left the card at "0 drafts · 2 releases". Both release directories were still on disk.
  - After restarting the app, 1.1.0 was still archived and not seeding.
- All repository gates pass.

## Current limitations

- There is a small race. A preparation started between the busy check and the delete can still publish a release whose draft is gone. The release itself is valid.
- Archiving cannot be undone in the UI, and an archived version cannot be republished.
- The publisher CLI has no `archive-release` or `delete-draft` command yet.
- Deleting an archived release's files is the next sub-slice.
