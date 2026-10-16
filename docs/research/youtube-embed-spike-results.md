# YouTube embed spike results

Date: 2026-10-16 · Platform: Linux x86_64, Hyprland (Wayland), WebKitGTK 2.52.6, GStreamer 1.28.6 · Tauri 2.12.4 · Code: `spikes/youtube-embed/` (throwaway, excluded from the workspace)

## Question

Can Zeron's Tauri webview host the YouTube IFrame Player API well enough to build keyless YouTube courses with timestamped notes? Specifically:

- Does the player load, play, report time, and seek?
- Which page origin does YouTube accept?
- Can a playlist be expanded without a Data API key?
- Can Rust fetch titles without a key?

## Setup

- A minimal Tauri app opens six windows: three page origins × two player hosts (`www.youtube.com` and `www.youtube-nocookie.com`). The three origins are:
  - **direct:** the player page is served from `tauri://localhost`, the app's own origin on Linux;
  - **loopback:** the window navigates to a player page served from `http://127.0.0.1:<port>`;
  - **nested:** a `tauri://localhost` page frames the loopback player page. This is how the real app would embed it.
- Each page loads `https://www.youtube.com/iframe_api` and creates a muted player for `M7lc1UVf-VE`. It plays for 3 s, seeks to 120 s, then cues the playlist `UU_x5XG1OV2P6uZZ5FSM9Ttw`. It posts every step to the loopback server, which prints it.
- Rust fetches `https://www.youtube.com/oembed?format=json&url=…` with `reqwest` (rustls) for a `watch` URL, a `youtu.be` URL, and a `playlist` URL.
- Reproduce: `cd spikes/youtube-embed && cargo run`. `SPIKE_ONLY=nested` limits the run to the windows whose label starts with that prefix.

## Results

| Scenario | Result |
|---|---|
| Media support in WebKitGTK | Pass. MSE is present; H.264, VP9 and AV1 report supported. |
| **direct** (`tauri://localhost`), both hosts | **Fails.** `onReady` fires with an empty title, then `onError` code **153**, YouTube's missing/invalid referrer error. YouTube does not accept the `tauri://` origin. |
| **loopback** (`http://127.0.0.1`), both hosts | Pass. The title and duration (1344 s) load, playback starts in about 1 s, `getCurrentTime()` advances, and `seekTo(120)` resumes at 120.2 s. |
| **nested** (a `tauri://` page framing the loopback page), both hosts | Pass, with the same numbers as loopback. The `tauri://` parent can frame `http://127.0.0.1`; WebKit treats loopback as a secure context, so it is not blocked as mixed content. |
| Keyless playlist expansion (`cuePlaylist` + `getPlaylist()`) | Pass, but the list is **capped at 200 video IDs** (the uploads playlist tested has more). |
| `youtube.com` vs `youtube-nocookie.com` | Same behaviour. |
| Keyless oEmbed from Rust | Pass for all three URL forms: title, author and thumbnail. A playlist URL returns the playlist's title. oEmbed returns no duration and no item list. |

## Findings and recommendations

1. **Serve the player page from loopback, never from `tauri://`.** The app's own page frames a small player page served by a Rust-owned loopback server. That server can be the existing media gateway or a sibling route. The app page and the player page talk by `postMessage` (play, seek, time updates, state).
2. **This keeps YouTube out of the main window's CSP.** Only the loopback player page loads `youtube.com` scripts and frames, under a CSP it sends itself. The main window's CSP only needs `frame-src` for the loopback origin. The rest of the app stays closed to the internet.
3. **Use `youtube-nocookie.com`.** It behaves the same and sets fewer cookies.
4. **Fetch titles with oEmbed from Rust** and keep the HTTP client behind the application boundary. Durations come from the player when a lesson first loads.
5. **Keyless playlists stop at 200 videos.** That is enough for nearly every course playlist. Show the limit in the UI, and leave the Data API key option for longer playlists.
6. **Untested, carried to the implementation slice:**
   - Windows (WebView2 page origin `http://tauri.localhost`) and macOS (`tauri://localhost`, probably the same error 153 as Linux). The nested approach should behave the same on both, because YouTube only sees the loopback referrer.
   - Fullscreen from inside the nested frame.
   - Captions.
   - The packaged AppImage, with its bundled GStreamer.
   - Videos whose owners disable embedding (error 101/150), which should show a clear "can't be embedded" state.

## Decision

The embed works: build YouTube courses on the nested loopback player. The next step is an ADR for online-only YouTube courses kept in learner data, then the single-video slice.
