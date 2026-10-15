# Phase 4: streaming torrent lessons through the gateway

Status: streaming is complete and verified against a real 31.7 GB course. In-app playback on Linux needed a follow-up: WebKitGTK refused the plain-HTTP gateway on the secure app page, which is fixed by `phase-4-linux-in-app-playback.md` (ADR 0005). Decoded playback and seeking were first confirmed in a Chromium-based browser against the same gateway URL.

## Delivered

- **Engine port (`torrent-port`):**
  - `TorrentEngine::open_file(info_hash, file_index)` returns a `VerifiedFile`, an async read-and-seek handle over one file of an active torrent, whether or not the file is selected.
  - Reads return only verified bytes and wait for missing pieces. The engine prioritizes pieces ahead of the read position, and dropping the reader releases that priority.
- **librqbit adapter (`torrent-librqbit`, ADR 0004 decision 7):**
  - Wraps librqbit's `FileStream`, which keeps a fixed 32 MiB window ahead of the read position and requests those pieces with priority even for unselected files. Opening a file requires an active transfer, and an index out of range is refused.
  - **Reader cap:** at most 4 readers per engine. Each librqbit stream holds one of the session's 8 blocking permits, which piece checks and writes also need.
  - **Reconnect workaround:** a torrent whose selection is complete disconnects its peers, and librqbit 9.0.1 can miss reconnecting a peer whose disconnect is still in progress when a stream opens. While a read waits, the reader opens and drops a short-lived stream every 3 s, which asks librqbit to reconnect needed peers again.
- **`torrent-metainfo`:** `file_table` lists files in declared order, which is the engine index order.
- **Media gateway (`media-gateway`):**
  - Reader-backed resources: `MediaSource` opens a fresh `MediaReader` per response, registered through `register_stream`. The gateway has no torrent dependency.
  - Ranges, HEAD, 416, authentication, the body frame timeout, and the global stream cap apply unchanged.
  - **Bounded waits:** each resource keeps at most two response bodies, and a newer request (for example after a seek) cancels the oldest, so abandoned waits do not pile up.
  - `revoke(url)` removes a capability and cancels its in-flight bodies. Shutdown cancels everything.
- **`learning-application`:**
  - `CourseLibrary::torrent_resources` maps each manifest resource to its torrent file by path and length.
  - `TransferRouter::open_file` reads from whichever engine holds the course.
- **Learner backend:**
  - `register_media` streams torrent lessons through the router as gateway stream resources. Registering a lesson revokes the previous one, so switching lessons cancels its pending waits.
  - `get_course_availability` reports the verified bytes of each torrent lesson on this computer.
- **Learner UI:**
  - Torrent lessons have Play buttons again, each showing "N% downloaded"; progress refreshes every 2 s while the course is open. The playing lesson is marked.
  - The course notice explains that lessons download while you watch and only data verified against the torrent is played.
  - The sharing card now says that only downloaded parts can be shared.

## Acceptance evidence

- **Adapter tests (loopback, real engines):**
  - **Seek without downloading the rest:** a 100 MB lesson published by Zeron, unselected on a learner with uploads off, is opened after `course.json` arrives. A read at 80 MB returns a byte-identical 1 MiB within 30 s, while less than 40 MB of the file is verified. A new reader from the start then returns the first MiB identically. Unknown torrents are `NotActive`; bad indexes are `InvalidTorrent`.
  - **Reconnect after the selection finishes:** the test opens a stream 0, 100, 200, 300, and 500 ms after `course.json` completes, and every read finishes. Before the fix, a scratch reproduction stalled at 300 ms, and the import test failed 2 of 5 runs. After it, 36 reproduction runs and 5 import runs all streamed.
- **Gateway tests:**
  - a stream resource waits for bytes and then serves an exact range;
  - a third request cancels the oldest waiting body while the newer two complete;
  - revoking cancels in-flight bodies and later requests are 404;
  - a source that fails to open is 503.
- **Application test:** after a magnet import, the lesson maps to its torrent file and streams bytes identical to the published package through the learner engine (uploads off).
- **`torrent-metainfo` test:** `file_table` equals the package files in path order and rejects an `info` without files.
- **Vitest:** a torrent lesson shows "25% downloaded", plays through an opaque gateway URL, and is marked Playing.
- **Real course and real apps:**
  - **Fixture:** the user's 122-lesson course (31.7 GB, fast-start H.264/AAC MP4s, 16 MiB pieces) was published by Zeron's pipeline into a scratch Admin data directory. The source folder was only read, and nothing from it is committed.
  - **Seeding:** the release-built Admin hash-checked the course and was seeding after 145 s.
  - **Import:** the release-built Learner (uploads off) imported it from the magnet in 3.1 s, with 17.3 MB on disk (`course.json` plus the 16 MiB piece it shares).
  - **Lesson 4 (168 MB),** through the URL the Learner issued: the first MiB arrived in 0.30 s and a seek to 80 % in 0.68 s, both byte-identical to the source. Lesson 4 then showed 21 % downloaded, Lesson 5 showed 0 %, and the course used 82 MB on disk.
  - **Lesson switch:** the previous URL returned 404, and 1 MiB from the middle of Lesson 77 (1.5 GB) arrived in 0.35 s, identical to the source.
  - **Restart:** Lesson 4 at 31 % and Lesson 77 at 2 % were still reported, and Lesson 4's first MiB was served in under 10 ms without downloading again.
  - **Decoded playback:** the Learner's WebKitGTK `<video>` reported error 4, format not supported, because no H.264 decoder is installed. The same gateway URL for Lesson 95 (653 MB, 1080p, 24 minutes, 0 % downloaded beforehand) played immediately in a Chromium-based browser on the same machine. A seek to 70 % was playing again after 7.2 s. The player buffered only 0–29 s and 1012–1037 s, Lesson 95 ended at 20 % downloaded with its neighbors at 0 %, and the whole course used 369 MB of the Learner's disk.
- All repository gates pass.

## Current limitations

- **Corrected after release:** the in-app failure was not only the missing decoder. WebKitGTK also refuses plain-HTTP media on the secure app page, and installing `gstreamer1.0-libav` alone still gave error 4. `phase-4-linux-in-app-playback.md` serves the gateway over HTTPS on Linux. Packaging must still declare an H.264/AAC decoder dependency.
- **Lookahead is librqbit's fixed 32 MiB window** with 16 MiB pieces on large courses. A seek downloads about three pieces before playback resumes (7.2 s on loopback in the check above). There is no configurable lookahead or verified-range API.
- **WebKitGTK's GStreamer player may read ahead much further than the browser does.** One lesson reached 64 % within seconds in the Learner window. Only that lesson is affected, never the rest of the course.
- **Not yet implemented:**
  - the architecture's playback state machine;
  - persisted playback position;
  - separate labels for player-buffered and verified ranges. The UI reports verified percentage per lesson only.
- **The reconnect nudge works around librqbit 9.0.1 behavior.** A recovered stream can take up to about 3 s longer to start.
- **Engine and gateway caps:** each engine allows 4 open readers and the gateway 8 concurrent bodies. A fifth reader on one engine fails, and the player retries.
- **Restart cost:** a restart re-checks every partly downloaded file, so partially streamed large lessons are re-hashed at startup.
