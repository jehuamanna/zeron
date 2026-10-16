# ADR 0018: Online-only YouTube courses in the Library

Status: accepted (2026-10-16)

## Context

The user wants to learn from YouTube videos and playlists, taking the same timestamped notes, sketches, and progress as in Zeron courses. They chose a keyless start: no YouTube Data API key.

The embed spike ([youtube-embed-spike-results.md](../research/youtube-embed-spike-results.md)) found:

- The YouTube IFrame player refuses the `tauri://localhost` page origin with error 153.
- The player works when its page is served from `127.0.0.1`, including inside a frame of the `tauri://` page.
- Keyless oEmbed returns titles.

The decision has to balance Zeron's existing rules:

- Zeron is offline-first, and its packages are immutable and distributed by BitTorrent.
- YouTube's terms forbid downloading, so YouTube videos can be watched only online, in YouTube's own player.
- The Tauri CSP and gateway changes are security-review triggers (AGENTS.md).
- The webview must not become a browser for remote content (ADR 0010).

## Decision

1. **A YouTube course is a library course that Zeron writes itself.**
   - Adding a link builds a schema-1 `course.json` in `learning-application` (`youtube_manifest`). It is checked with `course_format`'s `validate` like any other manifest and stored as a normal `courses` row.
   - Notes, typed video anchors (ADR 0009), drawings, progress, playback positions, outline search, the notebook, and removal (ADR 0015) therefore work unchanged.
   - For one video:
     - course ID `youtube-video-<videoId>`, version `1.0.0`, language `und`;
     - publisher: the channel name from oEmbed, else "YouTube";
     - a reference to `https://www.youtube.com/watch?v=<videoId>`;
     - one module with one lesson, `lesson-<videoId>`, holding one video resource, `video-<videoId>`.
   - The resource has no package bytes:
     - path `youtube/<videoId>` and media type `video/x-youtube`;
     - byte length 0 with the SHA-256 of empty content, so it is consistent;
     - no duration, which the player learns when it loads.
   - Adding the same video again refreshes the title and brings back the learner's kept work, like a re-import.
2. **The source is recorded, not inferred.**
   - Schema 16 adds `course_youtube_videos(course_id, resource_id, video_id)`. A YouTube course has no `course_locations` or `course_torrent_sources` row, so the local and torrent code paths find nothing to open, verify, index, or delete.
   - Saving a package course deletes any YouTube rows for that ID.
   - Adding a YouTube video refuses a course ID that a local or torrent course in the library already uses.
   - Removal keeps the YouTube rows, as it keeps the learner's work (ADR 0015). There is nothing on disk to delete, `youtube_video` ignores removed courses, and the notebook still knows a removed course came from YouTube.
3. **Links are parsed strictly in `learning-domain`** (`YouTubeLink::parse`).
   - Accepted: `https` (or schemeless) `youtube.com/watch?v=`, `m.`/`music.`/`www.` hosts, `youtu.be/<id>`, `/shorts/<id>`, `/live/<id>`, `/embed/<id>`, and the `-nocookie` embed host.
   - Video IDs are exactly 11 characters of `A–Z a–z 0–9 _ -`.
   - Playlist IDs are 2–64 characters of the same alphabet (`PlaylistId`).
   - Anything else is refused before any network request.
4. **Metadata through a port.**
   - `learning-application` defines `VideoMetadataSource`. The desktop adapter calls `https://www.youtube.com/oembed` with `reqwest`/rustls, a 10 s timeout, and a 64 KiB body cap.
   - Answers map to outcomes:
     - 401/403: the owner does not allow embedding;
     - 400/404: unavailable or private;
     - transport errors: offline.
   - Each refuses the add with its own message.
   - Titles and channel names are untrusted. They are trimmed, cut to 300 characters, and stripped of control characters.
5. **The player runs in a gateway page, not in the app's origin.**
   - `media-gateway` gains page resources: `register_page` serves fixed HTML from the same capability URL scheme as media. The headers are:
     - its own `Content-Security-Policy`;
     - `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin`, and `Cache-Control: no-store`.
   - A page URL is revoked like media when another lesson opens.
   - The player page's CSP is:
     - `default-src 'none'`;
     - the inline script by SHA-256 hash, plus `https://www.youtube.com` scripts;
     - frames only from `https://www.youtube-nocookie.com`;
     - `frame-ancestors` limited to Zeron's own origins.
   - `Referrer-Policy: strict-origin` sends YouTube only `https://127.0.0.1:<port>`, never the capability path. The video ID travels in the URL fragment, which is never sent.
   - The app's CSP only gains `frame-src` for the loopback gateway. YouTube is never loaded in the app's own origin.
   - The app and the player talk by `postMessage`:
     - the app checks the message's source window and gateway origin;
     - the player accepts commands only from its parent window.
6. **Online only, stated in the UI.**
   - YouTube courses show an "Online · YouTube" badge.
   - They offer no download, sharing, or offline controls.
   - The player says when YouTube cannot be reached or refuses a video.
   - Nothing from YouTube is downloaded or cached by Zeron.

7. **Playlists, keyless** (added 2026-10-16, slice 2).
   - **No other keyless source lists a playlist's videos:**
     - oEmbed returns only the playlist's title;
     - the RSS feed stops at 15 entries;
     - scraping YouTube's pages is fragile.
   - **So YouTube's own player lists them, out of sight:**
     1. `inspect_youtube_link` registers the same gateway page.
     2. The app frames the page with `#list=<id>` and hides it.
     3. YouTube's player cues the playlist, and the page posts `getPlaylist()`, at most 200 IDs, once.
     4. The app passes the IDs to `add_youtube_playlist`.
   - **The IDs are untrusted. Rust:**
     - re-parses each ID;
     - drops repeats, keeping each video's first place;
     - keeps at most 200;
     - looks each video up through oEmbed, 8 at a time, with progress events and a learner-cancellable flag.
   - **What is kept.** Private, removed, and non-embeddable videos are skipped and counted. An unreachable YouTube aborts the add. A playlist with nothing playable is refused.
   - **The course.**
     - Course ID `youtube-playlist-<listId>`, with one lesson per video in playlist order. Lesson IDs are `lesson-<videoId>`, so a video's notes follow it wherever it moves in the playlist.
     - The reference is the playlist page. The publisher is the playlist's channel.
   - **Refreshing.** Adding the playlist again refreshes it. When its set or order of lessons changed, the version goes up a patch.
     - The previous manifest goes into the manifest history.
     - The course page's existing "After an update" list (Phase 7 annotation moves) then shows notes whose video left the playlist, under the video's old title.
     - Nothing is deleted.
   - **One playlist add runs at a time.** Adding, cancelling, and starting another listing all revoke the listing page.

## Consequences

- The learner's notes on a YouTube video live in the same notebook and search as their other courses, and survive removing and re-adding the video.
- Without a network connection a YouTube lesson cannot play. Its notes, sketches, and progress stay readable.
- Video durations are unknown until a lesson first plays, so overviews show no duration for YouTube lessons.
- YouTube can change its embed rules. The spike app (`spikes/youtube-embed`) re-checks them.
- Untested platforms: Windows (origin `http://tauri.localhost`) and macOS. Their origins are listed in the player page's `frame-ancestors`, but they need manual verification.
- Playlists use the same model, as decision 7 describes.
