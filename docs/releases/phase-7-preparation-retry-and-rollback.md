# Phase 7: retrying and rolling back release preparations

Status: complete. A prepared release that should not be published can be discarded, which rolls the preparation back and removes its staged files. A failed preparation says what to fix and offers "Retry". A failed publish says how to recover.

## Delivered

- **`PublishJobs::discard(job_id)`** (`publisher-application`): accepts only a `READY_TO_PUBLISH` job. It removes the staged package and torrent and moves the job to `CANCELLED` with no prepared artifacts, so the draft is no longer busy and can be prepared again. Nothing is recorded. If removal fails, the job stays ready.
- **Activation and discarding are exclusive.** A ready job is claimed under the registry lock before either acts on its staged attempt. Activation releases the claim on failure, so a refused publish (for example a reused version) can still be discarded or retried.
- **Command:** `discard_publish_job { jobId } → PublishJob`. It emits the usual `publish-job` event. Failures use `preparation_discard_failed`.
- **UI (Prepare release panel):**
  - A ready release has "Discard" next to "Publish release".
  - A failed preparation shows advice for its failure code (`draft_invalid`, `manifest_invalid`, `source_changed`, `limit_exceeded`, `verification_failed`, or anything else), and the header button reads "Retry".
  - A failed publish explains `version_already_released` (discard, change the version, prepare again) and `release_activation_failed`.

## IPC compatibility

Additive: one command and one TypeScript API function.

## Acceptance evidence

- **Application tests** (`tests/publish_jobs.rs`):
  - `a_ready_preparation_can_be_discarded_and_prepared_again`: staging is emptied, the job is `CANCELLED` with a newer sequence, the draft is no longer busy, a second discard and an activation are refused, an unknown job is refused, and preparing again succeeds with a new job.
  - `a_failed_preparation_can_be_retried_once_its_cause_is_fixed`: a removed source fails as `draft_invalid` with no staged files, a failed job cannot be discarded, and restoring the file lets a retry reach `READY_TO_PUBLISH`.
  - The existing activation tests still pass with the claim in place, including a refused activation that stays ready.
- **Command test** `discarding_a_prepared_release_removes_its_staging_and_reports_codes`.
- **Vitest:** discard of a prepared release, failure advice with the Retry button, and the reused-version advice.
- All repository gates pass.

## Manual verification

1. Prepare a release, then choose "Discard". The panel should read "Cancelled", and the Courses page should allow deleting the draft.
2. Move a lesson file away and prepare. The failure should name the problem and offer "Retry". Put the file back and retry.

## Current limitations

- Preparations live only for the session. A ready preparation left at shutdown is removed by the existing startup recovery and cannot be published after a restart.
- Retry starts a new job from the current draft revision; it does not resume a half-finished one.
