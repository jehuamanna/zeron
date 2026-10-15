# Phase 4: lesson player states, errors, and buffered versus verified data

Status: complete. This covers Phase 4 items 1 (playback state machine) and 3 (separate "buffered" and "verified" labels). Under the video, the Learner shows whether a lesson is loading, buffering, playing, paused, stalled, finished, or failed. It also shows how far the player has buffered ahead, separately from how much of the lesson is verified on this computer. Failed lessons offer Retry.

## Delivered

- **`features/player/playbackState.ts`:** a pure reducer from media events to `loading`, `buffering`, `playing`, `paused`, `stalled`, `ended`, or `error`. It is built around what the real-course runs showed of WebKitGTK:
  - `readyState` stays at 2 while a stream plays, and `stalled` fires during normal playback. So a lesson counts as **playing** when `currentTime` advances, not when the element reports `playing`.
  - The jump caused by a seek is not counted as progress, so a seek stays **buffering** until playback moves on from the new position.
  - **Stalled** means buffering or loading with no `progress` for 8 s, measured by a watchdog tick. Progress returns it to buffering, and advancing time returns it to playing.
  - An **error** persists until the next load. Media error codes map to plain messages: stopped, stopped loading, cannot decode on this computer, or could not be loaded.
- **`LessonPlayer`:**
  - The status line has `aria-label="Playback status"` and `data-state`. It uses `role="status"`, or `role="alert"` on errors.
  - Next to it are "Buffered: m:ss ahead" (from the `buffered` range containing the current position) and, for torrent lessons, "Verified on this computer: N%" (from course availability).
  - Retry re-registers the lesson with the gateway.
- The Learner course view uses `LessonPlayer`.

## Acceptance evidence

- **Reducer tests:**
  - advancing time is required before playing;
  - a seek jump counts as buffering;
  - the stall watchdog runs, and `progress` resets it;
  - WebKit's spurious stalls are ignored while time advances;
  - errors are sticky and explained;
  - paused and finished are distinct states.
- **Component tests:** the loading → playing → buffering → stalled → playing sequence, with fake timers; the buffered and verified labels; an error alert with Retry re-registering; local lessons without a verified label.
- **Real window**, with the user's "Systems Design Interview Questions" course seeded by the Admin under the Local network profile:
  - Lesson 10 went loading → playing.
  - The Admin process was then frozen with `SIGSTOP` and the lesson seeked to 60%, into unfetched data. Within about 9 s the status read "Stalled: waiting for verified data from peers. It resumes on its own when data arrives.", with "Buffered: 0:00 ahead · Verified on this computer: 25%".
  - After `SIGCONT` the status returned to Playing within 1.5 s, and decoded frames advanced again.
  - This is first evidence for Phase 4 item 5 (peer interruption): the stream recovers when its only peer returns.
- All repository gates pass.

## Current limitations

- The 8 s stall threshold is fixed.
- The labels refresh with media events and the 2 s availability poll.
- Recovery after a peer disappears for good, as opposed to being frozen, and lookahead tuning are still part of Phase 4 item 5.
