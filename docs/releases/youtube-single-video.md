# YouTube courses, slice 1: learn from one YouTube video

Status: complete (2026-10-16). Paste a link to a YouTube video in the Library. It becomes an online-only course that plays in YouTube's own player inside Zeron. Timestamped notes, sketches, progress, resume, search, the notebook, and removal work as for any course. This follows [ADR 0018](../decisions/0018-youtube-courses.md) and the [embed spike](../research/youtube-embed-spike-results.md). No API key is used.

## Delivered

- **`learning-domain`:**
  - `YouTubeLink::parse` strictly parses the common video link forms (`watch`, `youtu.be`, `shorts`, `live`, `embed`, `m.`/`music.`, and the `-nocookie` embed host) and playlist links.
  - `VideoId` accepts exactly 11 characters of `A–Z a–z 0–9 _ -`.
  - `clean_youtube_text` makes YouTube's titles and channel names safe to show and store.
- **`learning-storage`, schema 16:**
  - The new table `course_youtube_videos(course_id, resource_id, video_id)`.
  - `save_youtube_course` stores a YouTube course without a location or torrent. It refuses a course ID that a folder or torrent course already uses (`CourseIdTaken`).
  - Saving a package course clears any YouTube rows for that ID.
  - `youtube_video` looks a video up and ignores removed courses. `course_is_youtube` covers removed courses too.
  - Summaries carry `youtube`. Removal keeps the YouTube rows with the learner's work (ADR 0015).
- **`learning-application`:**
  - `look_up_youtube_video` parses first, then asks a `VideoMetadataSource` port. Nothing is fetched for a link that is not one video, and playlists are refused for now.
  - `youtube_video_manifest` builds a schema-1 `course.json` that `course_format` validates:
    - one lesson `lesson-<id>` holding the video resource `video-<id>`;
    - media type `video/x-youtube`, 0 bytes, and the SHA-256 of empty content.
  - `CourseLibrary::add_youtube_video`, `youtube_video`, and `is_youtube_course`.
- **`media-gateway`:**
  - `register_page` serves a fixed HTML page under its own CSP, with `nosniff`, `Referrer-Policy: strict-origin`, and `no-store`, from the same revocable capability URLs as media.
  - `is_page_url` recognises exactly those pages.
- **Desktop:**
  - `youtube.rs` holds:
    - the oEmbed adapter (rustls, HTTPS only, no redirects, 10 s timeout, 64 KiB cap);
    - the player page, whose inline script and style are allowed by SHA-256 hash, whose frames come only from `https://www.youtube-nocookie.com`, and which only Zeron's own origins may frame.
  - The new command `add_youtube_video { link }` returns `ImportReceiptDto`. Its errors are `youtube_link_invalid`, `youtube_playlist_unsupported`, `youtube_not_embeddable`, `youtube_unavailable`, `youtube_unreachable`, `course_id_taken`, and `library_unavailable`.
  - `register_media` returns the player page URL with the video in the fragment, and media type `video/x-youtube`, for YouTube courses.
  - `source` is now `local`, `torrent`, or `youtube` in course summaries and details.
  - The navigation guard checks frames too on Linux. It now also allows the gateway's registered player page and `https://www.youtube-nocookie.com/embed/…`, and nothing else.
  - The app CSP gains `frame-src` for `127.0.0.1` only.
  - Base64 (`=0.22.1`, already in the lockfile) encodes the CSP hashes.
- **UI:**
  - The "Learn from a YouTube video" panel in Library → Add courses.
  - "Online · YouTube" on cards and the course overview, in place of the publisher badge.
  - `YouTubePlayer` frames the page and talks to it by `postMessage`. It checks the source window and gateway origin, and the page accepts commands only from its parent. It supports:
    - the same `PlayerHandle` as the lesson player (current time, seek, skip, play/pause, mute, volume), so K/J/L/←/→/↑/↓/M and N work;
    - "Note at mm:ss", note markers you can click to jump, saved positions with resume, and previous/next.
  - YouTube lessons have no transcript tab; YouTube shows its own captions.
  - The removal dialog says nothing is deleted from the computer.

## IPC and schema compatibility

The changes are additive:

- one new command;
- `CourseSource` gains `"youtube"`;
- `register_media` can return media type `video/x-youtube`.

Schema 16 adds one table. The downgrade-and-upgrade tests drop it with the other later tables.

## Acceptance evidence

- **Rust:**
  - Domain: link forms, refusals (look-alike hosts, ports, user info, wrong lengths, encoded slashes, other schemes), IDs, and text cleaning.
  - Storage:
    - `youtube_courses_have_no_location_and_keep_work_across_removal`;
    - `youtube_and_package_courses_never_share_an_id`.
  - Application:
    - the manifest round-trips through `parse_and_validate`;
    - IDs starting with `-` are handled;
    - empty titles get a name;
    - nothing is fetched for bad links or playlists;
    - notes and positions come back after removal and re-adding.
  - Gateway: `pages_are_served_whole_with_their_policy_and_can_be_revoked`, including `is_page_url`.
  - Desktop:
    - the player page CSP matches its script and style hashes;
    - only YouTube embed frames pass the guard;
    - the video travels in the fragment;
    - source names.
  - Ignored test that needs the internet: `oembed_describes_a_public_video` passed on 2026-10-16.
- **Vitest:**
  - `YouTubePlayer.test.tsx`: following reports, saving the position, ignoring other origins and windows, commands sent only to the gateway origin, resume, markers, notes, and error text.
  - `Library.test.tsx`: an add that fails and then succeeds, the card badge, and opening the lesson in a YouTube frame rather than a `<video>`.
- **Real window** (2026-10-16, Linux/Hyprland, WebKitGTK 2.52.6, `tauri-driver` 2.1.0, isolated `XDG_DATA_HOME`, app origin `tauri://localhost`):
  1. Pasting `https://www.youtube.com/watch?v=M7lc1UVf-VE` added "YouTube Developers Live: Embedded Web Player Customization" by Google for Developers.
  2. Opening it played from `https://127.0.0.1:<port>`, reading "Playing from YouTube 0:03 / 22:23".
  3. L moved to 0:15, and N opened a note at 0:15.
  4. A saved note at 0:18 survived restarting the app. The lesson resumed at 0:21 (`#v=…&t=21`), the note's marker showed, and clicking it went back to 0:18 and kept playing.
  5. The first run, before the guard change, showed that WebKitGTK sends subframe navigations through the guard: the frame stayed `about:blank`.

## Limitations

- **Online only.** Without a connection the player says YouTube cannot be reached. Notes, sketches, and progress stay readable.
- **One video per course.** Playlists are the next slice.
- Durations are unknown until a video first plays, so the overview shows no length for YouTube lessons.
- Zeron's speed, captions-from-transcript, and picture-in-picture controls are not offered; YouTube's own controls are. The learner's volume preference is not applied, only mute.
- YouTube's "watch on YouTube" links and other pop-ups do nothing, because the app has no new-window handler and stays a single window. Opening a video in the browser is left for later.
- **Untested:**
  - Windows and macOS. Their origins are in the player page's `frame-ancestors`, but they need a real-window check.
  - Fullscreen from the YouTube controls inside the nested frame.
  - The packaged AppImage.
- YouTube can change its embed rules. `spikes/youtube-embed` re-checks them.
