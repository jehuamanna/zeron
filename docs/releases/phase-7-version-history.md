# Phase 7: version history, rollback, and drafts from releases

Status: complete. Each course's release history now shows which version is current. A superseded version can be made current again, which rolls back a newer one, and any release whose files are present can start a new draft for the next version.

## Delivered

- **`publisher-storage`:** `make_release_current(release_id)` marks the course's current release superseded and the chosen superseded release published, in one transaction. A release that is already current is returned unchanged. Unknown and archived releases return `None`. There is no schema change.
- **`publisher-application`:**
  - `PublisherDrafts::make_release_current` returns `ReleaseArchived` for archived releases and `InvalidAttempt` for unknown ones.
  - `PublisherDrafts::draft_from_release(releases_root, release_id)` re-reads the release's `course.json` together with the exact package tree, and refuses a package whose manifest digest differs from the record or whose files were deleted. The new draft:
    - keeps the course, module, lesson, and resource IDs, so learners' notes, progress, and drawings stay attached to the same lessons in the next version;
    - uses the release's own read-only package files as sources, which staging only reads;
    - takes the next patch version (`1.2.0` → `1.2.1`) when the released version is semantic.
  - `StagedAttempt` keeps the validated manifest it already parsed.
- **Commands:** `make_release_current { releaseId } → Release` and `create_draft_from_release { releaseId } → DraftDetail`. Errors: `release_archived` and `release_unavailable`. No paths cross IPC.
- **UI (release details):**
  - The current release says so. A superseded release names the current version.
  - "Make current…" (superseded releases only) asks for confirmation and explains that no files change and seeding continues as it is.
  - "New draft from this version" (files present) creates the draft and opens it in the Builder.

## IPC and storage compatibility

Additive: two commands and two TypeScript API functions. DTOs and the publisher schema (v4) are unchanged.

## Acceptance evidence

- **Storage test** `a_superseded_release_can_become_current_again`: rollback across three releases with one archived, idempotence, refusals for archived and unknown releases, and exactly one published release per course afterwards.
- **Application tests:**
  - `an_older_release_can_be_made_current_and_archived_ones_cannot`;
  - `a_new_draft_from_a_release_keeps_its_ids_and_publishes_the_next_version`: same course, module, lesson, and resource IDs and package paths; sources inside the canonical release package; the draft is listed; preparing and activating it publishes `1.0.1`; a path-like id is refused;
  - `a_release_without_its_files_cannot_start_a_draft`.
- **Command test** `version_history_commands_roll_back_and_start_drafts_without_paths`: the draft DTO has version `1.0.1` and no scratch paths, rollback after a second release, `release_archived`, and `release_unavailable`.
- **Vitest:** make current with confirmation and the refreshed statuses; no rollback offered for the current or an archived release; a new draft opens in the Builder; a failure is shown and nothing opens.
- All repository gates pass.

## Manual verification

1. Publish versions 1.0.0 and 1.1.0 of a course.
2. Expand 1.0.0, choose "Make current…", and confirm. 1.0.0 should read published and 1.1.0 superseded.
3. Choose "New draft from this version" on 1.1.0. The Builder should open a draft at 1.1.1 with the same modules, lessons, and files. Prepare and publish it.

## Current limitations

- A draft started from a release depends on that release's files. If the release is later archived and its files deleted, preparing the draft fails with `source_changed` until its resources are re-attached.
- Making a release current changes only the publisher's record. Learners already holding a newer version keep it; nothing tells them about the rollback.
- The publisher CLI has no rollback or draft-from-release command.
