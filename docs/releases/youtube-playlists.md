# YouTube courses, slice 2: playlists

Status: complete (2026-10-16). Paste a YouTube playlist link in the Library. It becomes one online-only course with a lesson per video, in playlist order, up to 200 videos. Notes, progress, and resume work per video, and the player counts down to the next video. No API key is used. This follows decision 7 of [ADR 0018](../decisions/0018-youtube-courses.md) and builds on [slice 1](youtube-single-video.md).

## Delivered

- **`learning-domain`:** `PlaylistId` (2–64 characters of `A–Z a–z 0–9 _ -`). `YouTubeLink::Playlist` now carries it.
- **`learning-application`:**
  - `VideoMetadataSource::playlist` gives a playlist's title and channel.
  - `look_up_youtube_playlist` takes the untrusted listed IDs:
    - it drops malformed IDs and repeats, keeping first places, and keeps at most `MAX_PLAYLIST_VIDEOS` (200);
    - it looks each video up 8 at a time (`futures-util` `buffered`, so order is kept), reports progress, and honours a `CancellationFlag`;
    - it skips and counts private, removed, and non-embeddable videos;
    - it aborts when YouTube is unreachable, and refuses a playlist with nothing playable.
  - `youtube_playlist_manifest` builds the course. `CourseLibrary::add_youtube_playlist` adds or refreshes it, raising the patch version when the lessons changed. The single-video and playlist paths share `save_youtube_manifest`.
- **Desktop:**
  - The YouTube commands moved to `youtube.rs`:
    - `inspect_youtube_link { link }` returns `{ kind: "video" }` or `{ kind: "playlist", playlistId, pageUrl }`, and registers the listing page.
    - `add_youtube_video { link }` is unchanged.
    - `add_youtube_playlist { playlistId, videoIds }` returns the import receipt plus `skipped`. It emits `youtube-playlist-progress { playlistId, done, total }` and refuses a second add with `youtube_busy`.
    - `cancel_youtube_playlist` returns whether an add was running.
  - The new error codes are `youtube_playlist_empty`, `cancelled`, `youtube_busy`, and `youtube_playlist_link`. The last replaces `youtube_playlist_unsupported` and is now raised only when a playlist link reaches the video command.
  - The player page script has a `#list=<id>` mode. It cues the playlist and posts its video IDs once, polling `getPlaylist()`, or an empty list after 20 s. The CSP hash follows the script automatically.
  - oEmbed shares one HTTPS client for all of an add's lookups.
- **UI:**
  - "Learn from YouTube" accepts video and playlist links.
  - For a playlist:
    - it frames the listing page out of sight;
    - it accepts the list only from that frame and the gateway origin;
    - it shows "Checking videos: n of m" with a bar and Cancel;
    - it reports how many videos were left out.
  - Listing that takes longer than 25 s, an empty list, or a player error each give a clear message.
  - The YouTube player shows an "Up next" bar when a video ends, counting down when "Autoplay next lesson" is on.

## IPC compatibility

The changes are additive:

- three new commands;
- one event;
- the new TypeScript types `YouTubeLinkKind`, `PlaylistReceipt`, and `PlaylistProgress`.

The existing `add_youtube_video` contract is unchanged, apart from the error code it gives for a playlist link. There are no schema changes; playlist courses use `course_youtube_videos` from schema 16.

## Acceptance evidence

- **Rust:**
  - `playlists_keep_order_drop_bad_ids_and_repeats_and_skip_private_videos`;
  - `playlists_are_capped_and_fail_cleanly`: the 200 cap, empty and nothing-playable playlists, unreachable YouTube, and cancellation;
  - `refreshing_a_changed_playlist_raises_its_version_and_keeps_notes`: same videos keep 1.0.0, a changed set gives 1.0.1, the note is kept, and `annotation_moves` lists it under the video's old title;
  - domain `playlist_ids_are_short_safe_strings`;
  - desktop `link_answers_and_errors_are_path_free_and_coded`, and the playlist fragment URL.
- **Vitest:**
  - `YouTubeAddPanel.test.tsx`: listing through the hidden frame, ignoring other origins, filtering non-string IDs, progress, the skipped-videos message, Open it, empty and offline listings, Cancel, and the single-video path;
  - `YouTubePlayer.test.tsx`: the up-next countdown;
  - the `Library.test.tsx` flow updated for `inspect_youtube_link`.
- **Real window** (2026-10-16, Linux/Hyprland, WebKitGTK 2.52.6, `tauri-driver`, isolated `XDG_DATA_HOME`):
  - Pasting `https://www.youtube.com/playlist?list=UU_x5XG1OV2P6uZZ5FSM9Ttw` (Google for Developers uploads, more than 200 videos) worked:
    - the listing took 2.6 s;
    - the progress went from 1 to 200 of 200;
    - at 6.2 s it read "Added “Uploads from Google for Developers”: 200 videos."
  - The overview showed 200 lessons and "Online · YouTube".
  - Lesson 1 (`qJplHFjyv2A`) played. The Next lesson button opened lesson 2 (`i3X7Fv0Z3Uc`), which played: "0:00 / 1:27". These IDs match the order the spike listed.

## Limitations

- **At most 200 videos**, the most YouTube's keyless player lists. Longer playlists keep their first 200. A Data API key option would lift this.
- **Mixes and private playlists.** Mixes (`RD…`), "Watch later", and private playlists usually list nothing, and the learner is told so.
- **No automatic refresh.** Re-add the link to pick up changes.
- **Durations.** Each lesson's length is learned only when it first plays.
- **Not verified in the real window:**
  - the refresh-with-changes path, which the application test covers;
  - cancelling mid-way;
  - the up-next countdown at a real video's end.
- Windows, macOS, and the AppImage remain untested, as in slice 1.
