# Phase 7: importing and exporting the publisher configuration

Status: complete. Distribution → Publisher configuration exports trackers, followed tracker lists, and optionally the Network settings to a JSON file. Importing a file shows exactly what it would change before anything is applied.

## Delivered

- **File format** (`publisher-application::configuration`):
  - The file is `{ format: "zeron.publisher-configuration", version: 1, trackers, trackerLists, network? }`.
  - `network` holds `profile`, `peerPort`, `portForwarding`, `downloadLimitKib`, and `uploadLimitKib`.
- **`PublisherConfiguration::parse`** treats the file as untrusted. It enforces a 256 KiB bound, strict JSON with no unknown fields, the exact format and version, and the same validation as the settings screens:
  - `normalize_trackers` and `normalize_tracker_lists`;
  - a known profile;
  - a port of 1024 or higher;
  - speed limits of at least 16 KiB/s.
  - Errors name the field: `configuration`, `trackers`, `trackerLists`, `network.profile`, `network.peerPort`, or `network`.
- **Storing it:**
  - `PublisherDrafts::configuration(network)` builds the file contents.
  - `PublisherDrafts::apply_configuration` replaces trackers and lists. Published releases keep the trackers they carry.
- **Commands** (`publish::configuration`):
  - `export_publisher_configuration { includeNetwork } → bool`, through a native save dialog.
  - `choose_publisher_configuration → ConfigurationPreview | null` picks and validates a file and holds it behind a one-time token. The path stays in Rust.
  - `apply_publisher_configuration { token, applyNetwork } → { trackers, trackerLists, networkApplied, restartRequired }`. It stores the Network settings in the Library and applies the speed limits to the running engines at once. A changed profile or port reports `restartRequired`.
  - `discard_publisher_configuration { token }`.
  - Errors: `invalid_input` (with `field`), `configuration_unreadable`, `configuration_expired`, `export_failed`, `settings_failed`.
- **UI** (`ConfigurationSection`):
  - Export, with an "Include Network settings" choice.
  - Import, then a review panel listing the trackers, tracker lists, and Network settings in words. The publisher can leave the Network settings out, apply, or cancel.
  - A note warns that private tracker links can contain passkeys.

## IPC compatibility

Additive: four commands and new TypeScript types. No storage schema change.

## Acceptance evidence

- **Unit tests:** a round trip with and without network settings, and field-by-field refusal of non-JSON, another format, version 2, an unknown profile, port 80, a 1 KiB/s limit, a `file:` tracker, a plain-HTTP list, an unknown field, and an oversized file.
- **Command tests:**
  - A configuration exported from one publisher and Library imports into fresh ones. Without network, only trackers change. With network, the profile becomes Internet and `restartRequired` is reported, then not reported on a second identical apply.
  - The preview DTO is correct, and invalid, oversized, and missing files give the right codes.
  - Tokens apply once and only when current.
- **Vitest:** describing network settings in words, exporting with and without network, review then apply with the restart notice, leaving network out, cancel discarding the token, a cancelled picker, and an invalid file.
- All repository gates pass.

## Manual verification

1. On one computer, add a tracker, set Network to Internet with port 51413, and choose Distribution → Export….
2. On another, choose Import… and pick the file. Check the review, apply it, and restart Zeron when asked. The trackers and Network settings should match.

## Current limitations

- Tracker-list downloads are not exported; the importing computer downloads the lists again.
- Drafts, releases, and the activity log are not part of the configuration. Moving those is backup and restore, in Phase 10.
- Applying is not one transaction across the publisher and Library databases. If a write fails half way, the error is shown and applying again finishes it.
