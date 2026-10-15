# Phase 2: media inspection warnings without transcoding

Status: complete for on-demand, read-only compatibility checks and automatic duration capture.

## Delivered

- `course_publisher::inspect_media` performs a bounded, read-only inspection based on the declared media type:
  - **MP4 (`video/mp4`, `audio/mp4`):** a seek-based ISO-BMFF scan reads only box headers plus the small `mvhd`, `hdlr`, and `stsd` boxes, skipping sample tables and media data. Limits: depth 8, 4,096 boxes visited, 64 KiB per leaf box. Box sizes are validated against their parent, and 64-bit and to-end sizes are handled.
    - Warnings: index after media data (`mp4_not_fast_start`), no movie index (`mp4_index_missing`), no video track in a video file (`video_track_missing`), and content that is not an MP4 at all (`unrecognized_mp4`).
    - Advice: video codec other than H.264 (`avc1`/`avc3`) and audio codec other than AAC (`mp4a`).
    - The `mvhd` duration is rounded to whole seconds.
  - **WebM, QuickTime, and Matroska:** advice to prefer MP4 with H.264/AAC.
  - **SubRip:** advice to convert to WebVTT.
  - **PDF:** a warning when the file lacks the `%PDF-` signature.
  - Unreadable files produce no findings; validation already reports missing sources.
- Nothing is written, re-muxed, or transcoded. Finding messages are fixed strings and never include paths.
- Attaching a source now records the probed duration in the draft, which flows into `course.json` at staging.
- `publisher_application::inspect_draft_media(snapshot)` returns findings per resource with a draft field path and display file name, omitting clean resources. It works on a snapshot, so composition roots do not hold the draft lock while reading files.
- New Admin command `inspect_media(draftId)` returns flat `{ resourceId, field, fileName, code, severity: "warning" | "advice", message }` records. Resource DTOs gain `durationSeconds`.
- The readiness panel moved into `ReadinessPanel`. Its new "Media checks" section runs on demand ("Check media" / "Check again"), states that files are never changed, distinguishes warnings from advice with labelled icons, and clears stale results when the draft revision changes. Resource rows show durations such as `2:05` or `1:02:05`.

## Acceptance evidence

- Probe tests with synthetic containers: a fast-start H.264/AAC file has no findings and a 125-second duration, even with a 200 KB sample table that is skipped rather than read; the index after media data is a warning; HEVC/Opus yield two advice findings; missing index, non-MP4 bytes, an oversized box, and a fake PDF are warnings; WebM and SubRip get advice; WebVTT and real PDFs are clean; a missing file returns an empty report. 300 randomly mutated and truncated MP4s are inspected without a panic.
- Application tests: attaching a slow-start MP4 records 75 seconds (45,000 / 600), and draft inspection reports `mp4_not_fast_start` and `video_track_missing` with the right field path and file name. The PDF in the same lesson is clean, and the source bytes are unchanged.
- Admin command test: flattened DTO with `advice` severity, field path, and file name, no directory in the JSON, and `durationSeconds` on the resource DTO.
- Vitest: on-demand check with warning and advice rendering, results cleared on a new revision, a clean result, and duration formatting in the structure panel.
- Manual verification: in the browser pane with a fake IPC bridge, "Check media" listed one warning and two advice items under their file names, and resource rows showed `1:02:05` and sizes. Layout was correct at 800px width.
- All repository gates pass.

## Current limitations

- Only MP4-family containers are parsed. WebM, QuickTime, and Matroska get container-level advice without codec or duration probing; audio-only MP3 and other formats get no duration.
- Codec checks look at the first sample entry of each track type only; profiles, levels, bit depth, and resolution are not evaluated.
- Fragmented MP4 (`moof`) files are treated by their `moov` and `mdat` positions only.
- Durations of existing resources are not re-probed when the source file changes; re-attach to refresh.
- Optimization or re-encoding is intentionally unavailable. A future explicit operation must create a new derived file after consent.
