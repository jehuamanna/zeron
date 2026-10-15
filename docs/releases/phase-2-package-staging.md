# Phase 2: deterministic package staging and canonical `course.json`

Status: complete as a backend pipeline. The Admin publish workspace (progress, cancel, and result UI) arrives with torrent generation, so staged copies are not created before anything consumes them.

## Delivered

- `course_publisher::stage_package` turns a draft into a package directory:
  1. It rejects drafts with blocking validation issues.
  2. It enforces resource-count and total-size limits (defaults match the Learner importer: 10,000 resources, 4 TiB).
  3. It pre-validates the generated manifest with placeholder digests so version, language, and path errors fail before any copy.
  4. It creates a fresh destination and refuses to reuse an existing one, without touching it.
  5. It copies each source read-only with 1 MiB streaming SHA-256 and `create_new` writes, flushing each file.
  6. It rejects sources that grow or change during the copy.
  7. It writes canonical `course.json` and returns verified per-file facts and the manifest SHA-256.
  8. On any failure or cancellation it removes only the destination it created.
- `manifest_for` and `canonical_manifest_bytes` define the manifest mapping and serialization (ADR 0001). The license name maps to `license.name`; durations become seconds; positions follow draft order.
- `PackagingCancellation` is checked between files and copy chunks. Progress reports completed files and bytes after every chunk.
- Defense in depth: package paths are joined only from single, normal path segments, so a tampered draft cannot write outside the staging directory even if manifest validation were bypassed.
- `publisher_application::stage_draft` stages a frozen snapshot into `stage_<id>.partial`, renames it to `stage_<id>` on success, and returns a path-free `StagingReceipt` (attempt ID, draft revision, course ID and version, resource count, total bytes, manifest SHA-256). It takes a snapshot rather than storage, so composition roots can release the draft lock during long copies.
- `discard_abandoned_staging` removes only real `stage_<32 hex>.partial` directories directly under the staging root; completed attempts, unrelated names, and symbolic links are left alone.
- ADR 0001 records the package layout, canonical bytes, and attempt protocol.

## Acceptance evidence

- `course-publisher` packaging tests (real files):
  - A staged package is independently verified by the Learner's `import_local_course`, with matching manifest, lengths, and SHA-256.
  - Two stagings produce byte-identical `course.json` and resource trees.
  - Invalid drafts and invalid manifests (bad version plus a `../` path) fail before anything is written, and nothing appears outside the destination.
  - Cancellation removes the partial package.
  - A source that grows mid-copy is rejected.
  - An existing destination is neither reused nor removed.
  - Count and byte limits fail before copying.
  - Sources remain byte-identical.
- `publisher-application` staging tests: a draft built entirely through the service (metadata, structure, and a real file attached by path) stages into an attempt the Learner importer accepts. An unpublishable draft leaves no attempt. Abandoned-attempt cleanup removes exactly one matching partial directory and keeps completed attempts, unrelated directories, a matching-name symlink, and its target. The partial-name matcher accepts only the exact pattern.
- All repository gates pass.

## Current limitations

- The Admin app does not expose staging yet. The publish workspace with progress events, cancellation, startup cleanup of abandoned attempts, and the result receipt ships with torrent generation.
- Completed attempts are not yet retired; retention belongs to release activation.
- Media is not inspected (duration, codec, `moov` placement); declared media types come from extensions.
- Optional manifest fields without draft equivalents (references, thumbnail, relations, license URL, publisher ID, course duration) are omitted.
- Staging requires space for a full copy; free disk space is not checked before copying.
