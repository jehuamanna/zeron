# Phase 7: distribution activity log with redaction

Status: complete. Publish now keeps a local log of distribution events. The Distribution page shows it, each release's details show its own recent events, and "Export log…" saves the whole log as text. Paths, network addresses, and private parts of links are removed before anything is stored.

## Delivered

- **`publisher-storage` schema v5:**
  - New table `distribution_events (event_id, at, release_id, kind, detail)`, indexed by release.
  - `record_distribution_event` appends an event and prunes everything older than the newest `MAX_DISTRIBUTION_EVENTS` (2,000) in the same transaction.
  - `distribution_events(release_id?, limit)` lists events newest first.
- **`publisher-application`:**
  - `ActivityKind` has nine stable names: `release_published`, `release_made_current`, `release_archived`, `release_files_deleted`, `seeding_started`, `seeding_stopped`, `seeding_failed`, `tracker_lists_refreshed`, and `tracker_list_failed`.
  - `PublisherDrafts::record_activity` redacts and bounds the detail, and never fails the operation it describes. `PublisherDrafts::activity` lists events.
  - `redact` removes:
    - Unix, Windows drive, UNC, home-relative, and `file:` paths, including a path whose folder names contain spaces;
    - IPv4 and IPv6 addresses, with or without ports and brackets;
    - URL user information, paths other than `/` and `/announce`, queries, and fragments (private trackers put passkeys there), plus IP hosts in `udp:` URLs;
    - whole magnet links;
    - control characters. The result is bounded to 300 characters.
  - **Where events are recorded:**
    - Releases: activation, making a release current (only when it changes), archiving (only the first time), and file deletion.
    - Seeding: a start that actually began seeding, a stop that actually stopped it, and a failed start (a failure repeated with the same message is recorded once).
    - Tracker lists, from the desktop's refresh: one summary of the lists downloaded, and one event per list that failed.
- **Commands:**
  - `get_distribution_activity { releaseId | null } → ActivityEvent[]` returns the newest 200, each with the release's version.
  - `export_distribution_activity → bool` opens a native save dialog and writes every kept event, oldest first, one tab-separated line each with a UTC time. Errors: `activity_unavailable`, `export_failed`, `unsupported_file_location`.
- **UI:**
  - `ActivityLog` shows a full "Activity" section with "Export log…" on the Distribution page, and "Recent activity" (10 events) in release details.
  - Failures are highlighted, and the list refreshes every 5 s.

## IPC and storage compatibility

- **IPC:** two new commands and new TypeScript types (`ActivityEvent`, `ActivityKind`).
- **Storage:** migrating v4 to v5 creates the table. Older builds refuse a v5 database with `FutureSchema`; the v1 and v3 migration tests now migrate through v5.

## Acceptance evidence

- **Storage test** `distribution_events_are_listed_newest_first_and_pruned`: ordering, per-release filtering, the limit, and pruning to exactly 2,000 with the newest kept.
- **Redaction unit tests:** paths on every platform (including spaces), IPv4 and IPv6 with ports, URL secrets, `udp:` IP hosts, magnet links, ordinary text kept, control characters, and the length bound.
- **Application test** `distribution_activity_is_recorded_once_per_change_and_redacted`, against a scripted engine:
  - one `seeding_failed` for two identical failures, with its address and path redacted;
  - one `seeding_started` despite a repeated start, and one `seeding_stopped` despite a repeated stop;
  - archiving once, making current once, and file deletion;
  - ten events in total.
- **Command tests:** events carry versions and no scratch paths, tracker-list failures drop user information and queries, the export lists events oldest first, and UTC timestamps are correct around leap days.
- **Vitest:** the full list with versions, the per-release list without export, export cancel, success, and failure, and the empty state. The Courses and Distribution suites pass with the new section.
- All repository gates pass.

## Manual verification

1. Publish a release, start and stop seeding it, and refresh tracker lists. Distribution → Activity should list each event.
2. Choose "Export log…" and save. The file should contain one line per event, with no folder names or IP addresses.

## Current limitations

- Peer connections and per-peer traffic are not logged; the engine reports only counts.
- The log is kept on this computer and is not part of any backup or export of the publisher configuration.
- Redaction works on whitespace-separated words. The last folder name of a path that contains spaces can survive when it has no slash after it, for example `/home/ana/My Course` becomes `<path> Course`.
