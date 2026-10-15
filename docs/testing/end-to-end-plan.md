# End-to-end test plan

## Test environment

CI uses controlled local peers with no public trackers. Fixtures include a small v1 multi-file torrent, a root `course.json`, at least four fast-start MP4 lessons made from legally redistributable/generated media, WebVTT, PDF, Markdown, and deliberately corrupt variants. A deterministic network proxy introduces latency, bandwidth caps, disconnects, and corruption. Temporary directories and ports are isolated per test.

Unit tests cover schema invariants, path rules, state transitions, range parsing, piece/range mapping, anchor migration, deterministic ordering, and DTO serialization. Property/fuzz tests target manifests, bencode/metainfo ingestion boundaries, HTTP Range parsing, and path normalization. Integration tests use real SQLite and the real torrent adapter. UI E2E covers only business-critical flows.

## Mandatory publisher-to-learner scenario

1. Admin creates and autosaves a course with stable IDs and four lessons.
2. Validation detects a deliberately missing resource; after correction it passes.
3. Admin packages twice from identical frozen inputs; manifest and info dictionary are identical.
4. Admin generates and reparses a v1 torrent, verifies all pieces, publishes atomically, and seeds locally.
5. Learner imports the magnet, resolves metadata, and downloads only manifest/boundary pieces.
6. Learner sees the validated course tree, selects lesson four, and begins playback before that file completes.
7. Learner seeks into an unavailable region, buffers, and resumes without selecting unrelated lessons.
8. A timestamped note is durably saved.
9. Processes restart; torrent pieces, playback position, course identity, and note recover.
10. Admin publishes a new version; learner migration preserves the note or marks it for review without loss.

Assertions inspect HTTP status/headers, verified availability, selected file priorities, disk bytes, cancellation, DB records, UI labels, and absence of leaked secrets. Merely observing video playback is insufficient.

## Failure matrix

Required cases include incomplete magnet metadata, absent/oversized/invalid manifest, duplicate or unsafe paths, unsupported codec, `moov` placement warning, invalid/multiple ranges, unauthorized gateway requests, seek storms, rapid lesson switching, slow/no peers, peer loss, corrupt pieces, disk full, permission denial, source mutation during packaging, crash at every publishing stage, restart during active stream/seed, and graceful shutdown deadline.

## Platform matrix

The vertical slice runs on Windows WebView2, macOS WKWebView, and Linux WebKitGTK before streaming is declared supported. Test MP4/H.264/AAC and WebVTT first. Platform codec capability is feature-detected and unsupported files produce explicit errors. Packaging smoke tests cover each supported installer format; accessibility tests cover keyboard operation and drag/drop alternatives.

## Acceptance evidence

Every phase supplies automated results, manual verification notes, documented limitations, a demo script, and a completion report. Phase 0 captures engine/API versions and spike traces. Phase 4 captures evidence that playback starts before file completion, seek fetches absent verified pieces, memory remains bounded, cancellation removes obsolete work, and unrelated files are not deliberately downloaded.

