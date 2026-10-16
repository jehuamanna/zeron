//! YouTube for the Library (ADR 0018): video titles over keyless oEmbed, and
//! the gateway page that hosts YouTube's player outside the app's origin.

use std::{
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use base64::Engine as _;
use learning_application::{
    CancellationFlag, CourseImportReceipt, MAX_YOUTUBE_LINK_BYTES, PlaylistId, VideoDetails,
    VideoId, VideoLookupError, VideoMetadataSource, YouTubeAddError, YouTubeLink, YouTubeLinkError,
    look_up_youtube_playlist, look_up_youtube_video,
};
use media_gateway::{MediaGateway, PageResource};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tauri::{AppHandle, Emitter as _, State};

use crate::{
    AppState,
    commands::{CommandError, ImportReceiptDto, command_error, lock_library, run_blocking},
};

const PLAYLIST_PROGRESS_EVENT: &str = "youtube-playlist-progress";
/// More IDs than any playlist the keyless player lists, with room for junk.
const MAX_LISTED_IDS: usize = 1000;

const OEMBED_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OEMBED_BYTES: usize = 64 * 1024;

/// Titles and channel names from `https://www.youtube.com/oembed`, which
/// needs no API key. One client serves every lookup of an add.
pub(crate) struct OEmbed {
    client: Option<reqwest::Client>,
}

#[derive(Deserialize)]
struct OEmbedAnswer {
    title: String,
    #[serde(default)]
    author_name: Option<String>,
}

impl OEmbed {
    pub(crate) fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent(concat!("Zeron/", env!("CARGO_PKG_VERSION")))
                .timeout(OEMBED_TIMEOUT)
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .ok(),
        }
    }

    async fn describe(&self, page: &str) -> Result<VideoDetails, VideoLookupError> {
        let client = self.client.as_ref().ok_or(VideoLookupError::Unreachable)?;
        let request = url::Url::parse_with_params(
            "https://www.youtube.com/oembed",
            [("format", "json"), ("url", page)],
        )
        .map_err(|_| VideoLookupError::Unreachable)?;
        let mut response = client
            .get(request)
            .send()
            .await
            .map_err(|_| VideoLookupError::Unreachable)?;
        match response.status().as_u16() {
            200 => {}
            401 | 403 => return Err(VideoLookupError::NotEmbeddable),
            400 | 404 => return Err(VideoLookupError::Unavailable),
            _ => return Err(VideoLookupError::Unreachable),
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| VideoLookupError::Unreachable)?
        {
            if body.len() + chunk.len() > MAX_OEMBED_BYTES {
                return Err(VideoLookupError::Unreachable);
            }
            body.extend_from_slice(&chunk);
        }
        let answer: OEmbedAnswer =
            serde_json::from_slice(&body).map_err(|_| VideoLookupError::Unreachable)?;
        Ok(VideoDetails {
            title: answer.title,
            channel: answer.author_name,
        })
    }
}

impl VideoMetadataSource for OEmbed {
    async fn video(&self, id: &VideoId) -> Result<VideoDetails, VideoLookupError> {
        self.describe(&id.watch_url()).await
    }

    async fn playlist(&self, id: &PlaylistId) -> Result<VideoDetails, VideoLookupError> {
        self.describe(&id.playlist_url()).await
    }
}

const PLAYER_STYLE: &str =
    "html,body,#player{margin:0;width:100%;height:100%;background:#000;overflow:hidden}";

/// Runs inside the gateway page. It reads what to load from the URL
/// fragment, which is never sent to any server, and reports to the parent:
///
/// - `#list=<id>` cues a playlist and reports its video IDs once;
/// - `#v=<id>&t=<second>` plays a video, reports its time and state, and
///   takes commands only from the parent.
const PLAYER_SCRIPT: &str = r#"(() => {
  const params = new URLSearchParams(location.hash.slice(1));
  const id = params.get("v") || "";
  const start = Math.max(0, Math.floor(Number(params.get("t")) || 0));
  const list = params.get("list") || "";
  const send = (message) => window.parent.postMessage(Object.assign({ zeron: "youtube" }, message), "*");
  const loadApi = () => {
    const api = document.createElement("script");
    api.src = "https://www.youtube.com/iframe_api";
    api.onerror = () => send({ type: "error", code: "offline" });
    document.head.appendChild(api);
  };
  if (list) {
    if (!/^[A-Za-z0-9_-]{2,64}$/.test(list)) {
      send({ type: "error", code: "invalid" });
      return;
    }
    let sent = false;
    let lister = null;
    const finish = (videoIds) => {
      if (sent) return;
      sent = true;
      send({ type: "playlist", videoIds });
    };
    const check = () => {
      const ids = lister && typeof lister.getPlaylist === "function" ? lister.getPlaylist() : null;
      if (Array.isArray(ids) && ids.length) finish(ids.filter((value) => typeof value === "string"));
    };
    window.onYouTubeIframeAPIReady = () => {
      lister = new YT.Player("player", {
        host: "https://www.youtube-nocookie.com",
        width: "100%",
        height: "100%",
        playerVars: { listType: "playlist", list, origin: location.origin },
        events: {
          onReady: () => {
            check();
            const poll = setInterval(() => (sent ? clearInterval(poll) : check()), 500);
            setTimeout(() => finish([]), 20000);
          },
          onStateChange: check,
          onError: (event) => {
            if (!sent) send({ type: "error", code: event.data });
            sent = true;
          },
        },
      });
    };
    loadApi();
    return;
  }
  if (!/^[A-Za-z0-9_-]{11}$/.test(id)) {
    send({ type: "error", code: "invalid" });
    return;
  }
  let player = null;
  let timer = 0;
  const report = () => {
    if (!player || typeof player.getCurrentTime !== "function") return;
    send({ type: "time", position: player.getCurrentTime() || 0, duration: player.getDuration() || 0, state: player.getPlayerState() });
  };
  window.onYouTubeIframeAPIReady = () => {
    player = new YT.Player("player", {
      host: "https://www.youtube-nocookie.com",
      videoId: id,
      width: "100%",
      height: "100%",
      playerVars: { autoplay: 1, playsinline: 1, rel: 0, start, origin: location.origin },
      events: {
        onReady: () => {
          send({ type: "ready", duration: player.getDuration() || 0 });
          report();
        },
        onStateChange: (event) => {
          report();
          clearInterval(timer);
          if (event.data === YT.PlayerState.PLAYING) timer = setInterval(report, 500);
          if (event.data === YT.PlayerState.ENDED) send({ type: "ended" });
        },
        onError: (event) => send({ type: "error", code: event.data }),
      },
    });
  };
  window.addEventListener("message", (event) => {
    const command = event.data;
    if (event.source !== window.parent || !player || typeof command !== "object" || command === null || command.zeron !== "youtube") return;
    if (typeof player.getPlayerState !== "function") return;
    if (command.type === "seek" && Number.isFinite(command.seconds)) {
      player.seekTo(Math.max(0, command.seconds), true);
      report();
    } else if (command.type === "play") player.playVideo();
    else if (command.type === "pause") player.pauseVideo();
    else if (command.type === "toggle") {
      if (player.getPlayerState() === YT.PlayerState.PLAYING) player.pauseVideo();
      else player.playVideo();
    } else if (command.type === "mute") {
      if (player.isMuted()) player.unMute();
      else player.mute();
    } else if (command.type === "volume" && Number.isFinite(command.step)) {
      player.unMute();
      player.setVolume(Math.min(100, Math.max(0, player.getVolume() + command.step * 100)));
    }
  });
  loadApi();
})();"#;

/// Origins of Zeron's own pages, the only ones allowed to frame the player.
#[cfg(debug_assertions)]
const APP_ORIGINS: &str =
    "tauri://localhost http://tauri.localhost https://tauri.localhost http://localhost:1420";
#[cfg(not(debug_assertions))]
const APP_ORIGINS: &str = "tauri://localhost http://tauri.localhost https://tauri.localhost";

static PLAYER_PAGE: LazyLock<PageResource> = LazyLock::new(|| PageResource {
    html: format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>YouTube player</title><style>{PLAYER_STYLE}</style></head><body><div id=\"player\"></div><script>{PLAYER_SCRIPT}</script></body></html>"
    ),
    content_security_policy: format!(
        "default-src 'none'; script-src '{}' https://www.youtube.com; style-src '{}'; frame-src https://www.youtube-nocookie.com; img-src 'none'; connect-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors {APP_ORIGINS}",
        hash_source(PLAYER_SCRIPT),
        hash_source(PLAYER_STYLE),
    ),
});

fn hash_source(content: &str) -> String {
    format!(
        "sha256-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha256::digest(content.as_bytes()))
    )
}

/// The page that hosts the YouTube player, for the media gateway.
pub(crate) fn player_page() -> PageResource {
    PLAYER_PAGE.clone()
}

/// Whether `url` is YouTube's own player, which the gateway page frames:
/// exactly `https://www.youtube-nocookie.com/embed/…`.
pub(crate) fn is_embed_frame(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("www.youtube-nocookie.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path().starts_with("/embed/")
}

/// The player URL for one video: the page's capability URL with the video
/// in the fragment.
pub(crate) fn player_url(page_url: &str, video: &VideoId) -> String {
    format!("{page_url}#v={}", video.as_str())
}

/// The page URL that lists a playlist's videos.
pub(crate) fn playlist_url(page_url: &str, playlist: &PlaylistId) -> String {
    format!("{page_url}#list={}", playlist.as_str())
}

/// YouTube work in progress: the page listing a playlist, and the playlist
/// being added, which the learner can cancel. One at a time.
#[derive(Default)]
pub(crate) struct YouTubeState {
    list_page: Mutex<Option<String>>,
    adding: Mutex<Option<CancellationFlag>>,
}

impl YouTubeState {
    /// Forget the playlist page, revoking its capability URL.
    fn release_list_page(&self, gateway: &MediaGateway) {
        if let Some(page) = self.list_page.lock().ok().and_then(|mut page| page.take()) {
            gateway.revoke(&page);
        }
    }
}

/// What a pasted link is. For a playlist, the page that lists its videos.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum YouTubeLinkDto {
    Video {},
    Playlist {
        playlist_id: String,
        page_url: String,
    },
}

/// A playlist added to the library.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaylistReceiptDto {
    #[serde(flatten)]
    receipt: ImportReceiptDto,
    /// Videos left out because they are private, removed, or not embeddable.
    skipped: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistProgressDto {
    playlist_id: String,
    done: usize,
    total: usize,
}

/// Say whether a pasted link is a video or a playlist, fetching nothing. A
/// playlist gets a gateway page whose YouTube player lists its videos, for
/// the app to frame out of sight and pass to [`add_youtube_playlist`].
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects owned command arguments"
)]
pub(crate) fn inspect_youtube_link(
    link: String,
    state: State<'_, AppState>,
) -> Result<YouTubeLinkDto, CommandError> {
    if link.len() > MAX_YOUTUBE_LINK_BYTES {
        return Err(add_error(&YouTubeLinkError::NotYouTube.into()));
    }
    match YouTubeLink::parse(&link).map_err(|error| add_error(&error.into()))? {
        YouTubeLink::Video(_) => Ok(YouTubeLinkDto::Video {}),
        YouTubeLink::Playlist(playlist) => {
            state.youtube.release_list_page(&state.media_gateway);
            let page = state
                .media_gateway
                .register_page(player_page())
                .map_err(|error| command_error("media_registration_failed", error))?;
            if let Ok(mut current) = state.youtube.list_page.lock() {
                *current = Some(page.url.clone());
            }
            Ok(YouTubeLinkDto::Playlist {
                page_url: playlist_url(&page.url, &playlist),
                playlist_id: playlist.as_str().to_owned(),
            })
        }
    }
}

/// Add a YouTube video from a pasted link as an online-only course, or
/// refresh one added before (ADR 0018). The link is parsed before anything
/// is fetched; YouTube is asked only for the video's title and channel.
#[tauri::command]
pub(crate) async fn add_youtube_video(
    link: String,
    state: State<'_, AppState>,
) -> Result<ImportReceiptDto, CommandError> {
    if link.len() > MAX_YOUTUBE_LINK_BYTES {
        return Err(add_error(&YouTubeLinkError::NotYouTube.into()));
    }
    let course = look_up_youtube_video(&OEmbed::new(), &link)
        .await
        .map_err(|error| add_error(&error))?;
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        lock_library(&library)?
            .add_youtube_video(&course)
            .map(ImportReceiptDto::from)
            .map_err(|error| add_error(&error))
    })
    .await
}

/// Add a playlist, or refresh it, from the video IDs its YouTube player
/// listed. Every ID is checked again and every video is looked up, a few at
/// a time, with progress events; [`cancel_youtube_playlist`] stops it.
#[tauri::command]
pub(crate) async fn add_youtube_playlist(
    playlist_id: String,
    video_ids: Vec<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PlaylistReceiptDto, CommandError> {
    state.youtube.release_list_page(&state.media_gateway);
    let playlist = PlaylistId::parse(&playlist_id)
        .ok_or_else(|| add_error(&YouTubeLinkError::NotYouTube.into()))?;
    if video_ids.len() > MAX_LISTED_IDS || video_ids.iter().any(|id| id.len() > 64) {
        return Err(command_error(
            "invalid_input",
            "the playlist's video list is not valid",
        ));
    }
    let cancel = CancellationFlag::default();
    {
        let mut adding = state
            .youtube
            .adding
            .lock()
            .map_err(|_| command_error("youtube_busy", "another playlist is being added"))?;
        if adding.is_some() {
            return Err(command_error(
                "youtube_busy",
                "another playlist is being added",
            ));
        }
        *adding = Some(cancel.clone());
    }
    let found = look_up_youtube_playlist(
        &OEmbed::new(),
        &playlist,
        &video_ids,
        &cancel,
        |done, total| {
            let _ = app.emit(
                PLAYLIST_PROGRESS_EVENT,
                PlaylistProgressDto {
                    playlist_id: playlist_id.clone(),
                    done,
                    total,
                },
            );
        },
    )
    .await;
    if let Ok(mut adding) = state.youtube.adding.lock() {
        *adding = None;
    }
    let course = found.map_err(|error| add_error(&error))?;
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        lock_library(&library)?
            .add_youtube_playlist(&course)
            .map(|receipt: CourseImportReceipt| PlaylistReceiptDto {
                receipt: receipt.into(),
                skipped: course.skipped,
            })
            .map_err(|error| add_error(&error))
    })
    .await
}

/// Stop adding a playlist, and drop the page listing one. Returns whether a
/// playlist was being added.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects owned command arguments"
)]
pub(crate) fn cancel_youtube_playlist(state: State<'_, AppState>) -> bool {
    state.youtube.release_list_page(&state.media_gateway);
    state
        .youtube
        .adding
        .lock()
        .ok()
        .and_then(|adding| adding.as_ref().map(CancellationFlag::cancel))
        .is_some()
}

fn add_error(error: &YouTubeAddError) -> CommandError {
    let code = match error {
        YouTubeAddError::Link(_) => "youtube_link_invalid",
        YouTubeAddError::Playlist => "youtube_playlist_link",
        YouTubeAddError::EmptyPlaylist => "youtube_playlist_empty",
        YouTubeAddError::Cancelled => "cancelled",
        YouTubeAddError::Lookup(VideoLookupError::NotEmbeddable) => "youtube_not_embeddable",
        YouTubeAddError::Lookup(VideoLookupError::Unavailable) => "youtube_unavailable",
        YouTubeAddError::Lookup(VideoLookupError::Unreachable) => "youtube_unreachable",
        YouTubeAddError::CourseIdTaken => "course_id_taken",
        // Storage errors can name the database location; keep them in Rust.
        YouTubeAddError::Library(_) => {
            return CommandError {
                code: "library_unavailable",
                message: "the YouTube course could not be saved to the library".to_owned(),
            };
        }
    };
    command_error(code, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_player_page_allows_only_its_own_code_and_youtube() {
        let page = player_page();
        let policy = &page.content_security_policy;
        assert!(policy.starts_with("default-src 'none'; "));
        assert!(policy.contains(&format!(
            "script-src '{}' https://www.youtube.com;",
            hash_source(PLAYER_SCRIPT)
        )));
        assert!(policy.contains("frame-src https://www.youtube-nocookie.com;"));
        assert!(policy.contains("frame-ancestors tauri://localhost "));
        assert!(!policy.contains("unsafe"));
        // The hashed script and style are exactly what the page carries.
        assert!(
            page.html
                .contains(&format!("<script>{PLAYER_SCRIPT}</script>"))
        );
        assert!(
            page.html
                .contains(&format!("<style>{PLAYER_STYLE}</style>"))
        );
        assert_eq!(page.html.matches("<script").count(), 1);
        // A known vector: SHA-256 of the empty string.
        assert_eq!(
            hash_source(""),
            "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
        );
    }

    /// Asks the real YouTube; run manually with `--ignored`.
    #[tokio::test]
    #[ignore = "needs the internet"]
    async fn oembed_describes_a_public_video() {
        let video = VideoId::parse("M7lc1UVf-VE").unwrap();
        let details = OEmbed::new().video(&video).await.unwrap();
        assert!(details.title.contains("Embedded Web Player"));
        assert!(matches!(
            OEmbed::new()
                .video(&VideoId::parse("aaaaaaaaaaa").unwrap())
                .await,
            Err(VideoLookupError::Unavailable | VideoLookupError::NotEmbeddable)
        ));
    }

    #[test]
    fn only_youtube_embed_frames_are_let_through() {
        let parse = |value: &str| url::Url::parse(value).unwrap();
        assert!(is_embed_frame(&parse(
            "https://www.youtube-nocookie.com/embed/M7lc1UVf-VE?enablejsapi=1&origin=https%3A%2F%2F127.0.0.1%3A1"
        )));
        for refused in [
            "https://www.youtube.com/embed/M7lc1UVf-VE",
            "https://www.youtube-nocookie.com/watch?v=M7lc1UVf-VE",
            "http://www.youtube-nocookie.com/embed/M7lc1UVf-VE",
            "https://www.youtube-nocookie.com:8443/embed/M7lc1UVf-VE",
            "https://user@www.youtube-nocookie.com/embed/M7lc1UVf-VE",
            "https://www.youtube-nocookie.com.evil.example/embed/x",
        ] {
            assert!(!is_embed_frame(&parse(refused)), "{refused}");
        }
    }

    #[test]
    fn link_answers_and_errors_are_path_free_and_coded() {
        let playlist = serde_json::to_value(YouTubeLinkDto::Playlist {
            playlist_id: "PL123".to_owned(),
            page_url: "https://127.0.0.1:1/v1/media/h/s#list=PL123".to_owned(),
        })
        .unwrap();
        assert_eq!(playlist["kind"], "playlist");
        assert_eq!(playlist["playlistId"], "PL123");
        assert_eq!(
            playlist["pageUrl"],
            "https://127.0.0.1:1/v1/media/h/s#list=PL123"
        );
        assert_eq!(
            serde_json::to_value(YouTubeLinkDto::Video {}).unwrap()["kind"],
            "video"
        );
        assert_eq!(
            add_error(&YouTubeAddError::EmptyPlaylist).code,
            "youtube_playlist_empty"
        );
        assert_eq!(add_error(&YouTubeAddError::Cancelled).code, "cancelled");
        assert_eq!(
            add_error(&VideoLookupError::NotEmbeddable.into()).code,
            "youtube_not_embeddable"
        );
        let storage = add_error(&YouTubeAddError::Library(
            learning_application::LibraryError::NoteTarget("x"),
        ));
        assert_eq!(storage.code, "library_unavailable");
    }

    #[test]
    fn the_video_travels_in_the_fragment() {
        let video = VideoId::parse("M7lc1UVf-VE").unwrap();
        assert_eq!(
            player_url("https://127.0.0.1:1/v1/media/h/s", &video),
            "https://127.0.0.1:1/v1/media/h/s#v=M7lc1UVf-VE"
        );
        assert_eq!(
            playlist_url(
                "https://127.0.0.1:1/v1/media/h/s",
                &PlaylistId::parse("PL1").unwrap()
            ),
            "https://127.0.0.1:1/v1/media/h/s#list=PL1"
        );
    }
}
