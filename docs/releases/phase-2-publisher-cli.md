# Phase 2: publisher CLI parity

Status: complete for preparing, publishing, listing, and re-verifying releases from the command line over the Admin data-directory layout. This closes Phase 2.

## Delivered

- New tool `tools/publisher-cli`, a thin composition root over `publisher-application`:

  ```text
  publisher-cli <data-dir> drafts
  publisher-cli <data-dir> releases
  publisher-cli <data-dir> prepare <draft-id>
  publisher-cli <data-dir> publish <draft-id> --confirm
  publisher-cli <data-dir> verify-release <release-id>
  publisher-cli <data-dir> clean
  ```

  - `<data-dir>` must already exist and uses the same layout as the Admin app's data directory (`publisher.sqlite3`, `releases/`). On Linux that is `~/.local/share/app.zeron.publisher`.
  - `prepare` runs the full preparation job (validate, stage, hash, verify independently), prints stage progress on stderr and the receipt (files, bytes, pieces, info hash, magnet) on stdout, then discards the artifacts. It is a dry run.
  - `publish` refuses to run without `--confirm`. It rejects an already-released version before copying anything, then prepares and activates the release. If activation fails, its staged attempt is discarded.
  - `verify-release` performs a full re-verification of a recorded release.
  - The CLI stages in its own `cli-staging/` directory and never runs the Admin startup cleanup of `staging/`; `clean` removes only CLI staging entries.
- `PublisherDrafts::verify_release` re-checks a recorded release from its read-only files: exact package tree, manifest SHA-256 against the record, strict torrent structure, info hash against the record, and every piece hash. It reports progress and supports cancellation. This is also groundwork for the seeding-start check.
- Internal refactor: one package-inventory check (`StagedAttempt::open_package`) serves both staging attempts and release packages.

## Acceptance evidence

- Application test: a published release fully re-verifies with progress reaching its total byte count. Unknown, traversal-style, and unrecorded release IDs are rejected. Flipping one byte of a released resource (after lifting its read-only bit) fails piece verification.
- CLI integration tests run the built binary as a subprocess against a real data directory and real files:
  - `drafts` lists the draft.
  - The `prepare` dry run reports the receipt and the verifying stage, and leaves `cli-staging/` and `releases/` empty.
  - `publish` without `--confirm` fails and creates nothing.
  - `publish --confirm` creates exactly one release whose info hash equals the dry run's, showing determinism across runs.
  - `releases` shows it as published, `verify-release` passes, and the released package imports with the Learner importer.
  - Publishing the same version again fails before any staging output.
  - A missing draft, an unknown release, an unknown command, and a missing data directory each fail with clear messages.
  - `clean` removes only CLI staging and leaves an Admin attempt untouched.
- All repository gates pass.

## Current limitations

- The CLI cannot create or edit drafts; drafts are authored in the Admin app.
- Trackers and web seeds cannot be configured yet (trackerless torrents).
- Interrupting a CLI run can leave `cli-staging/*.partial`; run `clean` to remove it.
- Running the CLI's `publish` and the Admin app's publish at the same moment on one data directory is guarded only by the database's unique `(course, version)` constraint.
