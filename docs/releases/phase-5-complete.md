# Phase 5 complete: offline availability and transfer controls

Status: complete, except per-platform gaps noted below. Phase 5 delivered these slices:

| Slice | Release note |
|---|---|
| Engine file selection (ADR 0004 decision 9) | `phase-5-file-selection.md` |
| Keep lessons, modules, and courses offline, with availability labels (learner schema v6) | `phase-5-download-commitments.md` |
| Pause and resume per course (learner schema v7, ADR 0004 decision 10) | `phase-5-pause-and-resume.md` |
| Speed limits in both apps (ADR 0004 decision 11) | `phase-5-speed-limits.md` |
| Removing streamed lessons and a cache cap (ADR 0008) | `phase-5-cache-eviction.md` |
| Disk-pressure handling | `phase-5-disk-pressure.md` |

Per-course seeding preferences already existed as Phase 3's sharing toggle. Phase 5 adds per-course pause and session-wide speed caps on top.

## Fix found by the real-window check

"Free up space" restarted a course's transfer even when it had nothing to remove. That forced a full re-check (2.2 GB for the user's course), and kept lessons briefly read "Downloading 0%". Now:

- While a transfer runs, only lessons holding more than one piece of verified data are removed. Data shared at a file boundary frees almost nothing, and removing it would spoil the neighbouring lesson's piece.
- With nothing to remove, the transfer is left alone.
- The cache cap uses the same rule.
- The end-to-end test now checks that a removal with nothing to do does not restart the transfer.

## Real-window evidence

This run used the user's two courses: the Admin re-seeded both from stored intents after a restart, the Learner was driven by WebDriver, the Local network profile was active, and the data lived in scratch directories.

- **Pause:** while the course was paused, a lesson newly kept offline stayed at 0% at 3 s and at 12 s. After resuming it became "Available offline" within 2 s.
- **Removal:**
  - "Remove download" on a lesson that was 86% streamed and not kept took it to 0%, "Online only".
  - "Free up space" with nothing left to remove reported "Freed 0 MB" and left a kept lesson "Available offline", with no restart.
- **Download cap:** under a 1 MiB/s cap, a newly kept lesson of about 200 MB went from 0% to 5% in 10 s, about 1 MiB/s. Clearing the field restored no limit.
- **Storage:** the section showed "26.8 GB free on the downloads disk" and the streamed-lesson total.
- **Restart:** a lesson kept offline in an earlier run was still kept and available offline after restarting the Learner.

## Limitations carried forward

- Disk-pressure handling measures free space only on Unix.
- Speed caps are session-wide, not per course, and pausing is per course, not global.
- Removing a lesson invalidates pieces it shares with its neighbours; kept neighbours download them again.
- macOS and Windows have not been exercised.
