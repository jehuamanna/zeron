# Admin and publisher architecture

## User workflow

The Admin Desktop has five workspaces:

1. **Courses** lists drafts and immutable releases separately.
2. **Builder** edits course metadata and the Course → Module → Lesson → Resource tree with keyboard-accessible drag/drop and explicit save status.
3. **Validation** groups blocking errors, warnings, and media compatibility advice with navigation back to the source field.
4. **Publish** previews the package plan, tracker settings, version, generated artifacts, and irreversible immutability boundary.
5. **Distribution** operates local seeding jobs and reports engine-local metrics with their source and timestamp.

Draft autosave writes normalized draft records transactionally. Adding a resource records an opaque resource ID and a backend-owned source-file reference; it does not copy or mutate bytes until packaging. Preview resolves the draft through the same validator used by publishing.

## Publishing use case

Publishing runs as a cancellable backend job in a newly created staging directory:

```text
DRAFT -> VALIDATING -> PACKAGING -> GENERATING_TORRENT
      -> VERIFYING -> READY_TO_PUBLISH -> PUBLISHED -> SEEDING
```

Terminal/side states are `FAILED`, `CANCELLED`, `SUPERSEDED`, and `ARCHIVED`. `FAILED` records the last completed stage and a structured, safe error; retry creates a new attempt. Cancellation is legal before atomic activation. Once `PUBLISHED`, payload mutation is forbidden.

| From | Preconditions | Durable result |
|---|---|---|
| DRAFT → VALIDATING | draft snapshot exists; no publish job active | frozen input revision |
| VALIDATING → PACKAGING | no blocking validation issues | validation report |
| PACKAGING → GENERATING_TORRENT | staged package and manifest hashes complete | package inventory |
| GENERATING_TORRENT → VERIFYING | torrent and magnet artifacts parse | candidate info hash |
| VERIFYING → READY_TO_PUBLISH | torrent maps exactly to staged files; piece hashes pass | verification report |
| READY_TO_PUBLISH → PUBLISHED | destination free; artifacts fsynced where supported | atomic release activation and release row |
| PUBLISHED → SEEDING | release directory intact; engine accepts torrent in seed mode | seeding job ID |
| PUBLISHED → SUPERSEDED | a different verified release is designated current | old release retained |
| any mutable stage → CANCELLED | cancellation requested | staging cleanup eligibility |
| any stage → FAILED | classified error | diagnostic and retry eligibility |

External catalog publication is another explicitly confirmed operation and never changes local `PUBLISHED` truth. Removing a seeding job leaves release files intact. Deleting release bytes requires a separate destructive action and confirmation.

## Packaging pipeline

`course-publisher` accepts a frozen draft, resource resolver, packaging options, and tracker configuration. It validates safe relative paths and symlink policy; creates a normalized package in staging; emits canonical `course.json`; probes but never silently transcodes media; generates v1 multi-file metainfo; reparses it; verifies file inventory, lengths, piece hashes, and info hash; then emits a publishing report.

Determinism applies to the manifest and info dictionary for identical normalized inputs and options. Non-info metadata such as creation time must either be omitted, supplied explicitly, or excluded from determinism claims. File order is bytewise-defined and piece size selection is deterministic.

Progressive media checks recommend MP4/H.264/AAC with the `moov` atom before media data and WebVTT subtitles. An explicit optimization operation may produce a new derived file after preview and consent; the publisher source is never overwritten.

## Frontend state

TanStack Query owns persisted backend data and invalidation. Zustand owns transient builder selection, panel layout, and unsaved interaction state. Draft mutations use revision-based optimistic concurrency: stale saves are rejected rather than last-write-wins. Job progress is read from snapshots plus throttled events.

## Failure and recovery

Staging directories include an attempt record. Startup detects abandoned attempts and offers retry or safe cleanup. Activation uses rename on the same filesystem; cross-volume destinations require copy → verify → activate. Disk-full, permission, source mutation, missing files, and engine failures remain recoverable and cannot yield `PUBLISHED`.

