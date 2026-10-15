# Phase 4: the first streaming milestone as automated evidence

Status: complete. This covers Phase 4 items 4 and 5. Two loopback-only adapter tests make the roadmap's first milestone repeatable in CI without any public network. A third, from the startup slice, covers streaming while a torrent is still checking.

## Delivered

- **`the_first_streaming_milestone_holds_in_a_controlled_swarm`** (`torrent-librqbit/tests/seeding.rs`). A five-lesson release (5 × 24 MB) is seeded by a Loopback engine. A learner with uploads off fetches only `course.json`. Then:
  1. **Lesson four plays before it is complete.** The first 1 MiB of `lesson-3.bin` streams and matches the source, with less than half of the lesson verified.
  2. **A seek into absent data resumes.** Reading 1 MiB at 90%, which was not yet verified, succeeds once it is fetched.
  3. **A restart reuses verified bytes.** The learner and the seeder both shut down. A new learner engine on the same state and content directories, with no peer at all, re-checks and reports at least as many verified bytes for lesson four as before. It streams both previously read regions offline.
- **`a_stream_survives_losing_one_of_two_seeders`** covers item 5, peer interruption.
  - Two seeders serve one 40 MB lesson, and the learner is connected to both. 4 MiB is streamed before the test checks that.
  - One seeder then shuts down. The rest of the lesson streams from the other within the time limit and matches the source byte for byte.
- **Already in place:**
  - Lookahead is librqbit's `FileStream` priority window, about 32 MiB ahead of the read position (ADR 0004). `streams_verified_bytes_from_any_offset_without_downloading_the_rest` shows a far seek fetches only that window.
  - `a_stream_opened_while_the_torrent_is_still_checking_waits_for_it` (`phase-4-streaming-at-startup.md`) covers opening a stream during the initial check.

## Acceptance evidence

- Both new tests passed three runs in a row, in about 9 s each. Real-window runs with the user's courses (`phase-4-streaming-at-startup.md`, `phase-4-player-states.md`) showed the same behaviour, including recovery after the only seeder was frozen and resumed.
- All repository gates pass.

## Current limitations

- The lookahead window is librqbit's fixed default. Tuning it per bitrate is not implemented.
- A learner whose only seeder leaves for good waits until a peer appears. The player reports this as "Stalled".
- macOS and Windows playback (Phase 4 item 6) need those machines and are still unverified.
