//! YouTube links a learner pastes, parsed strictly before anything is fetched
//! (ADR 0018).

use thiserror::Error;
use url::Url;

/// Longest link accepted, in bytes.
pub const MAX_LINK_BYTES: usize = 2048;

/// A YouTube video ID: exactly 11 characters of `A–Z a–z 0–9 _ -`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VideoId(String);

impl VideoId {
    /// Accept exactly an 11-character YouTube video ID.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        (value.len() == 11
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
        .then(|| Self(value.to_owned()))
    }

    /// The ID as YouTube writes it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The canonical watch page, as a link back to the source.
    #[must_use]
    pub fn watch_url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.0)
    }
}

/// A YouTube playlist ID: 2–64 characters of `A–Z a–z 0–9 _ -`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlaylistId(String);

impl PlaylistId {
    /// Accept a playlist ID of safe characters.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        ((2..=64).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
        .then(|| Self(value.to_owned()))
    }

    /// The ID as YouTube writes it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The canonical playlist page, as a link back to the source.
    #[must_use]
    pub fn playlist_url(&self) -> String {
        format!("https://www.youtube.com/playlist?list={}", self.0)
    }
}

/// What a pasted link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YouTubeLink {
    /// One video.
    Video(VideoId),
    /// A playlist.
    Playlist(PlaylistId),
}

/// Why a link was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum YouTubeLinkError {
    /// Not a link to a YouTube video or playlist.
    #[error("this is not a link to a YouTube video or playlist")]
    NotYouTube,
}

const VIDEO_HOSTS: [&str; 4] = [
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
];
const EMBED_HOSTS: [&str; 2] = ["youtube-nocookie.com", "www.youtube-nocookie.com"];

impl YouTubeLink {
    /// Parse a pasted link. A link with both a video and a playlist, as
    /// YouTube writes when a video plays inside a playlist, means the video.
    ///
    /// # Errors
    ///
    /// Returns [`YouTubeLinkError::NotYouTube`] for anything else.
    pub fn parse(input: &str) -> Result<Self, YouTubeLinkError> {
        let input = input.trim();
        if input.is_empty() || input.len() > MAX_LINK_BYTES {
            return Err(YouTubeLinkError::NotYouTube);
        }
        let with_scheme = if input.contains("://") {
            input.to_owned()
        } else {
            format!("https://{input}")
        };
        let url = Url::parse(&with_scheme).map_err(|_| YouTubeLinkError::NotYouTube)?;
        if !matches!(url.scheme(), "https" | "http")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return Err(YouTubeLinkError::NotYouTube);
        }
        let host = url
            .host_str()
            .ok_or(YouTubeLinkError::NotYouTube)?
            .to_ascii_lowercase();
        let segments: Vec<&str> = url
            .path_segments()
            .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
            .unwrap_or_default();
        let query = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };
        let video = |value: &str| {
            VideoId::parse(value)
                .map(Self::Video)
                .ok_or(YouTubeLinkError::NotYouTube)
        };

        if host == "youtu.be" {
            return match segments.as_slice() {
                [id] => video(id),
                _ => Err(YouTubeLinkError::NotYouTube),
            };
        }
        if EMBED_HOSTS.contains(&host.as_str()) {
            return match segments.as_slice() {
                ["embed", id] => video(id),
                _ => Err(YouTubeLinkError::NotYouTube),
            };
        }
        if !VIDEO_HOSTS.contains(&host.as_str()) {
            return Err(YouTubeLinkError::NotYouTube);
        }
        match segments.as_slice() {
            ["watch"] => match query("v") {
                Some(id) => video(&id),
                None => playlist(query("list").as_deref()),
            },
            ["playlist"] => playlist(query("list").as_deref()),
            ["shorts" | "live" | "embed" | "v", id] => video(id),
            _ => Err(YouTubeLinkError::NotYouTube),
        }
    }
}

fn playlist(list: Option<&str>) -> Result<YouTubeLink, YouTubeLinkError> {
    list.and_then(PlaylistId::parse)
        .map(YouTubeLink::Playlist)
        .ok_or(YouTubeLinkError::NotYouTube)
}

/// Make untrusted text from YouTube safe to show and store: no control
/// characters, single spaces, trimmed, and at most `max_chars` characters.
#[must_use]
pub fn clean_youtube_text(value: &str, max_chars: usize) -> String {
    let spaced: String = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    spaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "M7lc1UVf-VE";

    fn video(id: &str) -> YouTubeLink {
        YouTubeLink::Video(VideoId::parse(id).unwrap())
    }

    #[test]
    fn video_links_in_every_common_form_give_the_video() {
        for link in [
            "https://www.youtube.com/watch?v=M7lc1UVf-VE",
            "http://youtube.com/watch?v=M7lc1UVf-VE&t=42s",
            "www.youtube.com/watch?feature=share&v=M7lc1UVf-VE",
            "  https://m.youtube.com/watch?v=M7lc1UVf-VE  ",
            "https://music.youtube.com/watch?v=M7lc1UVf-VE",
            "https://youtu.be/M7lc1UVf-VE",
            "youtu.be/M7lc1UVf-VE?si=abc",
            "https://www.youtube.com/shorts/M7lc1UVf-VE",
            "https://www.youtube.com/live/M7lc1UVf-VE?feature=x",
            "https://www.youtube.com/embed/M7lc1UVf-VE",
            "https://www.youtube-nocookie.com/embed/M7lc1UVf-VE",
            "HTTPS://WWW.YOUTUBE.COM/watch?v=M7lc1UVf-VE",
            // A video playing inside a playlist means the video.
            "https://www.youtube.com/watch?v=M7lc1UVf-VE&list=PL1234567890",
        ] {
            assert_eq!(YouTubeLink::parse(link), Ok(video(ID)), "{link}");
        }
    }

    #[test]
    fn playlist_links_give_the_list() {
        for link in [
            "https://www.youtube.com/playlist?list=UU_x5XG1OV2P6uZZ5FSM9Ttw",
            "https://www.youtube.com/watch?list=UU_x5XG1OV2P6uZZ5FSM9Ttw",
        ] {
            assert_eq!(
                YouTubeLink::parse(link),
                Ok(YouTubeLink::Playlist(
                    PlaylistId::parse("UU_x5XG1OV2P6uZZ5FSM9Ttw").unwrap()
                )),
                "{link}"
            );
        }
    }

    #[test]
    fn anything_else_is_refused() {
        for link in [
            "",
            "   ",
            "not a link",
            "https://example.com/watch?v=M7lc1UVf-VE",
            "https://youtube.com.evil.example/watch?v=M7lc1UVf-VE",
            "https://evilyoutube.com/watch?v=M7lc1UVf-VE",
            "https://www.youtube.com/watch?v=short",
            "https://www.youtube.com/watch?v=M7lc1UVf-VE1",
            "https://www.youtube.com/watch?v=M7lc1UVf%2FE",
            "https://www.youtube.com/watch",
            "https://www.youtube.com/@GoogleDevelopers",
            "https://www.youtube.com/playlist?list=",
            "https://www.youtube.com/playlist?list=a/b",
            "https://youtu.be/",
            "https://youtu.be/M7lc1UVf-VE/extra",
            "https://user@www.youtube.com/watch?v=M7lc1UVf-VE",
            "https://www.youtube.com:8443/watch?v=M7lc1UVf-VE",
            "ftp://www.youtube.com/watch?v=M7lc1UVf-VE",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://www.youtube-nocookie.com/watch?v=M7lc1UVf-VE",
        ] {
            assert_eq!(
                YouTubeLink::parse(link),
                Err(YouTubeLinkError::NotYouTube),
                "{link}"
            );
        }
        let long = format!("https://youtu.be/{ID}?pad={}", "a".repeat(MAX_LINK_BYTES));
        assert_eq!(YouTubeLink::parse(&long), Err(YouTubeLinkError::NotYouTube));
    }

    #[test]
    fn video_ids_are_exactly_eleven_safe_characters() {
        assert!(VideoId::parse("-_abcDEF012").is_some());
        assert!(VideoId::parse("abcDEF0123").is_none());
        assert!(VideoId::parse("abcDEF01234").is_some());
        assert!(VideoId::parse("abcDEF012345").is_none());
        assert!(VideoId::parse("abcDEF0123.").is_none());
        assert!(VideoId::parse("abcDEF0123é").is_none());
        assert_eq!(
            VideoId::parse(ID).unwrap().watch_url(),
            "https://www.youtube.com/watch?v=M7lc1UVf-VE"
        );
    }

    #[test]
    fn playlist_ids_are_short_safe_strings() {
        assert!(PlaylistId::parse("PL").is_some());
        assert!(PlaylistId::parse("P").is_none());
        assert!(PlaylistId::parse(&"a".repeat(64)).is_some());
        assert!(PlaylistId::parse(&"a".repeat(65)).is_none());
        assert!(PlaylistId::parse("PL a").is_none());
        assert_eq!(
            PlaylistId::parse("PL123").unwrap().playlist_url(),
            "https://www.youtube.com/playlist?list=PL123"
        );
    }

    #[test]
    fn youtube_text_is_cleaned_and_cut() {
        assert_eq!(
            clean_youtube_text("  A\u{0}title\n\twith  gaps ", 300),
            "A title with gaps"
        );
        assert_eq!(clean_youtube_text("ééééé", 3), "ééé");
        assert_eq!(clean_youtube_text("\u{7}\u{1b}", 10), "");
    }
}
