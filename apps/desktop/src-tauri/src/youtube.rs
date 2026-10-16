//! YouTube for the Library (ADR 0018): video titles over keyless oEmbed, and
//! the gateway page that hosts YouTube's player outside the app's origin.

use std::{sync::LazyLock, time::Duration};

use base64::Engine as _;
use learning_application::{VideoDetails, VideoId, VideoLookupError, VideoMetadataSource};
use media_gateway::PageResource;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

const OEMBED_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OEMBED_BYTES: usize = 64 * 1024;

/// Titles and channel names from `https://www.youtube.com/oembed`, which
/// needs no API key.
pub(crate) struct OEmbed;

#[derive(Deserialize)]
struct OEmbedAnswer {
    title: String,
    #[serde(default)]
    author_name: Option<String>,
}

impl VideoMetadataSource for OEmbed {
    async fn video(&self, id: &VideoId) -> Result<VideoDetails, VideoLookupError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("Zeron/", env!("CARGO_PKG_VERSION")))
            .timeout(OEMBED_TIMEOUT)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| VideoLookupError::Unreachable)?;
        let request = url::Url::parse_with_params(
            "https://www.youtube.com/oembed",
            [("format", "json"), ("url", id.watch_url().as_str())],
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

const PLAYER_STYLE: &str =
    "html,body,#player{margin:0;width:100%;height:100%;background:#000;overflow:hidden}";

/// Runs inside the gateway page. It reads the video ID and start second from
/// the URL fragment, which is never sent to any server, reports the player's
/// time and state to the parent, and takes commands only from the parent.
const PLAYER_SCRIPT: &str = r#"(() => {
  const params = new URLSearchParams(location.hash.slice(1));
  const id = params.get("v") || "";
  const start = Math.max(0, Math.floor(Number(params.get("t")) || 0));
  const send = (message) => window.parent.postMessage(Object.assign({ zeron: "youtube" }, message), "*");
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
  const api = document.createElement("script");
  api.src = "https://www.youtube.com/iframe_api";
  api.onerror = () => send({ type: "error", code: "offline" });
  document.head.appendChild(api);
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
        let details = OEmbed.video(&video).await.unwrap();
        assert!(details.title.contains("Embedded Web Player"));
        assert!(matches!(
            OEmbed.video(&VideoId::parse("aaaaaaaaaaa").unwrap()).await,
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
    fn the_video_travels_in_the_fragment() {
        let video = VideoId::parse("M7lc1UVf-VE").unwrap();
        assert_eq!(
            player_url("https://127.0.0.1:1/v1/media/h/s", &video),
            "https://127.0.0.1:1/v1/media/h/s#v=M7lc1UVf-VE"
        );
    }
}
