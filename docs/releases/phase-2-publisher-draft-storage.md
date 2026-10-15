# Phase 2: publisher draft storage

Status: complete for durable draft snapshots and autosave concurrency.

## Delivered

- `publisher-storage` owns a dedicated migrated `SQLite` database for mutable Admin drafts.
- Stable draft IDs are independent from logical course IDs and future release IDs.
- New drafts begin at revision zero; each autosave must advance exactly one revision.
- Updates use a database compare-and-swap so a stale editor cannot overwrite a newer snapshot.
- Exact draft snapshots recover after process restart while list results contain no source paths.
- Databases created by a newer Zeron version are rejected instead of being silently modified.

## Acceptance evidence

- File-backed recovery is tested across database close and reopen.
- Competing-editor coverage proves the first autosave survives a stale second write.
- Skipped revisions and future schema versions are rejected explicitly.
- Workspace lint and test gates cover the new migration and serialization boundary.

## Current limitations

- Source file paths are confined to backend draft snapshots; Admin IPC must expose only opaque source references or display-safe names.
- Draft deletion and duplication are deferred until the Admin application service can enforce confirmation and identity rules.
- Immutable release records remain separate and will be introduced with the packaging/activation slice.
