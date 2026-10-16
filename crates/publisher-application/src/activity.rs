//! Redacted, bounded distribution activity for publishers.

use std::net::{IpAddr, SocketAddr};

use publisher_storage::DistributionEvent;

use crate::{DraftServiceError, PublisherDrafts};

/// Longest detail kept for one event, in characters.
pub const MAX_ACTIVITY_DETAIL_CHARS: usize = 300;
const PATH: &str = "<path>";
const ADDRESS: &str = "<address>";

/// What happened in a distribution event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    /// A prepared release was published.
    ReleasePublished,
    /// A superseded release became the course's current release again.
    ReleaseMadeCurrent,
    /// A release was archived; seeding stopped for good.
    ReleaseArchived,
    /// An archived release's files were removed.
    ReleaseFilesDeleted,
    /// The engine started seeding a release.
    SeedingStarted,
    /// Seeding a release was stopped.
    SeedingStopped,
    /// A release could not be seeded.
    SeedingFailed,
    /// Followed tracker lists were downloaded.
    TrackerListsRefreshed,
    /// A followed tracker list could not be downloaded.
    TrackerListFailed,
}

impl ActivityKind {
    /// Stable machine-readable name, as stored and exposed over IPC.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReleasePublished => "release_published",
            Self::ReleaseMadeCurrent => "release_made_current",
            Self::ReleaseArchived => "release_archived",
            Self::ReleaseFilesDeleted => "release_files_deleted",
            Self::SeedingStarted => "seeding_started",
            Self::SeedingStopped => "seeding_stopped",
            Self::SeedingFailed => "seeding_failed",
            Self::TrackerListsRefreshed => "tracker_lists_refreshed",
            Self::TrackerListFailed => "tracker_list_failed",
        }
    }
}

impl PublisherDrafts {
    /// Record a distribution event. The detail is redacted and bounded
    /// first. Recording never fails the operation it describes, so storage
    /// errors are dropped.
    pub fn record_activity(&mut self, release_id: Option<&str>, kind: ActivityKind, detail: &str) {
        let detail = redact(detail);
        let _ = self
            .storage
            .record_distribution_event(release_id, kind.as_str(), &detail);
    }

    /// Recorded distribution events, newest first, for one release or all.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn activity(
        &self,
        release_id: Option<&str>,
        limit: u64,
    ) -> Result<Vec<DistributionEvent>, DraftServiceError> {
        Ok(self.storage.distribution_events(release_id, limit)?)
    }
}

/// Remove what could identify the publisher's machine or accounts from a
/// diagnostic: filesystem paths, IP addresses, `file:` URLs, and the user
/// information, path, query, and fragment of other URLs (private trackers
/// put passkeys there). Control characters become spaces and the result is
/// bounded to [`MAX_ACTIVITY_DETAIL_CHARS`].
#[must_use]
pub fn redact(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let mut words: Vec<String> = Vec::new();
    let mut inside_path = false;
    for word in cleaned.split(' ').filter(|word| !word.is_empty()) {
        let (lead, core, trail) = split_punctuation(word);
        if inside_path && (core.contains('/') || core.contains('\\')) {
            if !trail.is_empty() {
                inside_path = false;
                if let Some(last) = words.last_mut() {
                    last.push_str(trail);
                }
            }
            continue;
        }
        inside_path = false;
        let replacement = if is_path(core) {
            inside_path = trail.is_empty();
            Some(PATH.to_owned())
        } else if is_address(core) {
            Some(ADDRESS.to_owned())
        } else if core.contains("://") || core.starts_with("magnet:") {
            Some(redact_url(core))
        } else {
            None
        };
        match replacement {
            Some(replacement) => {
                // An IPv6 address keeps no half of its brackets.
                let lead = if replacement == ADDRESS {
                    lead.trim_end_matches('[')
                } else {
                    lead
                };
                words.push(format!("{lead}{replacement}{trail}"));
            }
            None => words.push(word.to_owned()),
        }
    }
    let joined = words.join(" ");
    if joined.chars().count() > MAX_ACTIVITY_DETAIL_CHARS {
        let mut bounded: String = joined.chars().take(MAX_ACTIVITY_DETAIL_CHARS - 1).collect();
        bounded.push('…');
        bounded
    } else {
        joined
    }
}

fn split_punctuation(word: &str) -> (&str, &str, &str) {
    let opening = |character: char| matches!(character, '"' | '\'' | '`' | '(' | '[' | '<');
    let closing = |character: char| {
        matches!(
            character,
            '"' | '\'' | '`' | ')' | ']' | '>' | ',' | ';' | ':' | '.'
        )
    };
    let start = word.len() - word.trim_start_matches(opening).len();
    let rest = &word[start..];
    let core = rest.trim_end_matches(closing);
    (&word[..start], core, &rest[core.len()..])
}

fn is_path(word: &str) -> bool {
    let bytes = word.as_bytes();
    let windows_drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    word.starts_with("file:")
        || word.starts_with("\\\\")
        || word.starts_with("~/")
        || word.starts_with("~\\")
        || windows_drive
        || (word.starts_with('/') && word.len() > 1 && !word.starts_with("//"))
}

fn is_address(word: &str) -> bool {
    // A leading `[` was split off as punctuation, so `[::1]:80` arrives as
    // `::1]:80` and `[::1]` as `::1`.
    word.parse::<IpAddr>().is_ok()
        || word.parse::<SocketAddr>().is_ok()
        || format!("[{word}").parse::<SocketAddr>().is_ok()
        || looks_like_ipv4_with_port(word)
}

fn looks_like_ipv4_with_port(word: &str) -> bool {
    let host = word.split(':').next().unwrap_or(word);
    host.split('.').count() == 4
        && host.split('.').all(|part| {
            !part.is_empty() && part.len() <= 3 && part.bytes().all(|b| b.is_ascii_digit())
        })
}

fn redact_url(word: &str) -> String {
    if word.starts_with("magnet:") {
        return "<magnet link>".to_owned();
    }
    let Ok(url) = url::Url::parse(word) else {
        return "<link>".to_owned();
    };
    if url.scheme() == "file" {
        return PATH.to_owned();
    }
    let Some(host) = url.host() else {
        return format!("{}:<link>", url.scheme());
    };
    let host = match host {
        // Non-special schemes such as `udp:` keep IP hosts as opaque text.
        url::Host::Domain(domain) if !is_address(domain.trim_matches(['[', ']'])) => {
            domain.to_owned()
        }
        url::Host::Domain(_) | url::Host::Ipv4(_) | url::Host::Ipv6(_) => ADDRESS.to_owned(),
    };
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let path = match url.path() {
        "" | "/" => "",
        "/announce" => "/announce",
        _ => "/…",
    };
    let hidden = if url.query().is_some() || url.fragment().is_some() {
        "?…"
    } else {
        ""
    };
    format!("{}://{host}{port}{path}{hidden}", url.scheme())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_of_every_platform_are_removed() {
        assert_eq!(
            redact("cannot open /home/ana/Courses/Rust/lesson1.mp4 for reading"),
            "cannot open <path> for reading"
        );
        assert_eq!(
            redact("failed: C:\\Users\\Ana\\Videos\\a.mp4."),
            "failed: <path>."
        );
        assert_eq!(redact("at \\\\nas\\share\\x"), "at <path>");
        assert_eq!(redact("in '~/Library/x'"), "in '<path>'");
        assert_eq!(redact("see file:///tmp/a"), "see <path>");
        assert_eq!(
            redact("open /home/ana/My Course/files/a.mp4 failed"),
            "open <path> failed"
        );
    }

    #[test]
    fn addresses_and_url_secrets_are_removed() {
        assert_eq!(
            redact("peer 192.168.1.20:51413 left"),
            "peer <address> left"
        );
        assert_eq!(
            redact("peer [2001:db8::1]:6881 left"),
            "peer <address> left"
        );
        assert_eq!(redact("from 10.0.0.1"), "from <address>");
        assert_eq!(
            redact("tracker https://user:pw@tracker.example:8443/a1b2c3/announce?passkey=x"),
            "tracker https://tracker.example:8443/…?…"
        );
        assert_eq!(
            redact("http://tracker.example/announce failed"),
            "http://tracker.example/announce failed"
        );
        assert_eq!(redact("udp://93.184.216.34:80"), "udp://<address>:80");
        assert_eq!(redact("magnet:?xt=urn:btih:abc&tr=x"), "<magnet link>");
    }

    #[test]
    fn ordinary_text_is_kept_and_bounded() {
        assert_eq!(redact("version 1.2.0 published"), "version 1.2.0 published");
        assert_eq!(redact("ratio 1/2 and 3.5"), "ratio 1/2 and 3.5");
        assert_eq!(redact("line\nbreak\ttab"), "line break tab");
        let long = "word ".repeat(200);
        let bounded = redact(&long);
        assert_eq!(bounded.chars().count(), MAX_ACTIVITY_DETAIL_CHARS);
        assert!(bounded.ends_with('…'));
    }
}
