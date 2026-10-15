# Phase 6: Lesson transcripts

Status: complete. While a video or audio lesson plays, its transcript appears below the player. The line being spoken is highlighted and kept in view, clicking a line seeks to it, and the lines can be filtered. Transcripts are read through the same verified paths as PDFs, and parsed in Rust into plain-text cues, so no subtitle markup reaches the window.

## Delivered

- **Domain (`learning-domain::transcript`):** `parse_transcript` reads WebVTT and SubRip into `Cue { start, end, text }`, sorted by start.
  - **Input:** UTF-8, up to 4 MiB and 50,000 cues. A byte-order mark and CR/CRLF line endings are accepted.
  - **Skipped blocks:** the header and `NOTE`, `STYLE` and `REGION` blocks.
  - **Timings:** cue identifiers and SubRip indices may precede the timing line, and cue settings after it are ignored. Timestamps must be well formed with three-digit milliseconds, and a cue may not end before it starts.
  - **Text:** tags (`<b>`, `<v Speaker>`, inline timestamps, SubRip `{\an8}`) are removed and WebVTT entities are decoded.
  - **Unreadable cues:** they are skipped. A file with none is refused (`NoCues`), and non-UTF-8 or oversized input is refused (`NotText`).
- **Application:** `CourseLibrary::transcript_source(course, media)` picks the media's transcript in this order:
  1. a `transcript` relation;
  2. a `subtitle` relation;
  3. the first `transcript` resource of the same lesson;
  4. the first `subtitle` resource of the same lesson.

  Only video and audio resources have transcripts. `document_source` still returns PDFs only.
- **Command:** `read_transcript { courseId, mediaResourceId }` returns `{ resourceId, language, cues }` or `null`.
  - **Reads:** local files are re-hashed against the manifest, and torrent files come through the verified stream. This read path is now shared with `read_document`.
  - **Errors:** an oversized file returns `document_too_large`, and unreadable files return `transcript_unreadable`.
- **UI (`TranscriptPanel`):** shown under the player for the playing resource, unless a PDF is open.
  - It follows the player every 250 ms, using a binary search over cues. The active line is marked `aria-current` and scrolled into view inside the list only.
  - The "Follow video" toggle and the "Search transcript" filter work as their names say; a filter with no matches says so.
  - Media without a transcript shows nothing, and unreadable transcripts show an alert.

## Acceptance evidence

- **Domain tests:**
  - A WebVTT file with a BOM, CRLF line endings, a NOTE, a STYLE, a cue identifier, settings, voice and bold tags, entities and short `mm:ss.mmm` timestamps parses correctly.
  - SubRip with `{\an8}` and out-of-order cues is sorted.
  - Backwards cues, bad seconds, short milliseconds and empty cues are skipped.
  - Empty and plain-text files, invalid UTF-8 and oversized input are refused.
- **Application test:**
  - A video's `subtitle` relation (fr) wins over the lesson's transcript (en), and a video without relations gets the lesson's transcript.
  - PDFs, transcripts themselves, unknown resources and unknown courses have no transcript.
  - A transcript is not a PDF document.
- **Vitest:**
  - The active cue is found at boundaries and across hours.
  - The panel follows the time, including `1:01:40`.
  - A line seeks to its start, and filtering narrows the lines or reports no match.
  - Media without a transcript renders nothing, and an unreadable transcript shows its alert.
- **Real window** (WebKitGTK, a scratch local course of `lesson1.mp4` from the seed folder with a generated 40-cue WebVTT linked by a `transcript` relation):
  - "Transcript (en)" showed 40 lines, and line 1 was active at 1.5 s.
  - Seeking to 120 s made line 23 (2:00) active.
  - Clicking line 5 moved the video to 0:21 and made that line active.
  - Filtering "replication" left 13 lines.
- All repository gates pass.

## Current limitations

- The Admin builder does not yet link subtitles to videos, so for those courses the Learner falls back to the lesson's first transcript or subtitle. Only one transcript is shown, with no language choice.
- There are no native captions over the video, because a `<track>` would need a media-source URL for the text.
- Transcripts are not yet part of offline search. That comes with the FTS5 slice.
