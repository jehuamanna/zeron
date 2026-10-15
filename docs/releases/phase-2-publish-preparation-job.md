# Phase 2: Admin release preparation job

Status: complete for preparing verified release artifacts (staged package plus torrent) from the Admin app as one cancellable, observable job. Activating those artifacts into an immutable release is the next slice.

## Delivered

- `publisher_application::PublishJobs` registers and runs release preparation:
  - **Stages:** Validating → Packaging → GeneratingTorrent → Verifying → ReadyToPublish, enforced by the existing `PublishingWorkflow`, ending in Failed or Cancelled where applicable.
  - **One job at a time:** a second `begin` returns `JobActive`. Each job freezes a draft snapshot and its revision.
  - **Snapshots:** each change increments a monotonic `sequence`. Notifications carry the same snapshot as a hint, and the registry snapshot remains the source of truth. Progress is throttled to one update per 200 ms plus completion, and stage changes are always reported.
  - **Cancellation:** a shared token is checked across staging, hashing, and verification. Cancelled or failed jobs remove their own staging attempt and torrent; nothing partial survives.
  - **Failures:** path-free codes (`draft_invalid`, `manifest_invalid`, `source_changed`, `limit_exceeded`, `verification_failed`, `preparation_failed`) with the underlying message.
  - **Retention:** the 16 most recent finished jobs stay queryable. `recover()` runs `discard_abandoned_staging` and is called on every Admin startup.
- `generate_torrent` reports a separate verification phase (`TorrentPhase::Verifying`), so the job shows the Verifying stage without hashing a third time. `discard_attempt` removes an exact attempt and its torrent.
- Admin backend: `AppState` owns `PublishJobs` with app-data `staging/`. New commands:
  - `start_publish_preparation(draftId)` snapshots the draft under the storage lock, releases the lock, and runs the job on a blocking worker;
  - `get_publish_job(jobId)` returns the current snapshot;
  - `cancel_publish_job(jobId)` requests cancellation.

  Snapshots are emitted as `publish-job` events. DTOs use `SCREAMING_SNAKE_CASE` stages and expose only opaque IDs, hashes, the magnet, and sizes. No capability changed (`core:default` already covers event listening).
- Admin UI: a "Prepare release" panel under the readiness panel.
  - It is disabled while blocking issues exist and keeps a five-step checklist with the current step, a percentage, and an accessible progress bar.
  - It offers Cancel while running and shows "Cancelled. Partial files were removed." or the failure message.
  - On success it shows version, file count and size, piece count and size, the info hash, and a selectable magnet link, and flags results prepared from an older draft revision.
  - Job IDs are remembered per draft in the Zustand builder store. Snapshots live in TanStack Query: refetched every 1.5 s while running as a fallback, with events applied only when their `sequence` is newer.
- Byte and duration formatting moved to `src/lib/format.ts` for reuse.

## Acceptance evidence

- Application job tests (real files, a 3 MiB source):
  - stage order exactly Validating → Packaging → GeneratingTorrent → Verifying → ReadyToPublish, with strictly increasing sequence numbers and completed packaging progress;
  - the final snapshot equals the registry snapshot, and the attempt directory and torrent exist;
  - a finished job cannot be cancelled, and only one job may run at a time;
  - an invalid draft fails at validation with `draft_invalid` and no artifacts;
  - cancellation at Packaging, GeneratingTorrent, and Verifying each leaves the staging root empty and allows a new job;
  - startup recovery removes partial attempts.
- Admin command test: a real preparation run serialized through the DTO, with stages `VALIDATING` … `READY_TO_PUBLISH`, a 40-character info hash, two files, a null failure, and no directory paths in the JSON.
- Vitest (`PublishPanel`): event-driven progress and step tracking, a stale lower-sequence event ignored, the verified result card, the blocked state, cancel plus cancelled and failed displays, and the older-revision warning. Three consecutive full runs were stable.
- Manual verification:
  - In the browser pane, with a fake IPC bridge that also emulated Tauri's event channel, a simulated run showed each step with live percentages (for example "Verify torrent independently 60%") and ended in the result card with "1401 × 512 KB", the info hash, and the magnet. Cancelling a second run at 35% of packaging showed the cancelled message. There was no horizontal overflow at 1009px.
  - The real debug binary started with an abandoned `stage_<hex>.partial` directory planted under its app-data staging root; startup recovery removed it and the app kept running.
- All repository gates pass.

## Current limitations

- Jobs live in memory. After a restart, a finished preparation is no longer listed and its completed attempt stays on disk until release activation adds durable records and retention.
- Trackers and web seeds are not configurable yet; preparations are trackerless.
- The native webview flow was not driven end to end by automation. The Rust job path is covered with real files, and the UI with component tests plus the browser-pane run.
- Progress covers bytes within the current stage; there is no overall time estimate.
