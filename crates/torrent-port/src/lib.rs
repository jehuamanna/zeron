//! Engine-neutral torrent capability port.
//!
//! Applications depend on these types only. Engine adapters translate them and
//! must never expose engine-specific types, paths of other torrents, or
//! unverified bytes.

use std::{fmt, future::Future, net::SocketAddr, path::PathBuf};

use thiserror::Error;

/// Lowercase hexadecimal v1 info hash.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InfoHash(String);

impl InfoHash {
    /// Parse a 40-character hexadecimal info hash, normalizing to lowercase.
    ///
    /// # Errors
    /// Returns [`EngineError::InvalidTorrent`] for anything else.
    pub fn parse(value: &str) -> Result<Self, EngineError> {
        if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            Ok(Self(value.to_ascii_lowercase()))
        } else {
            Err(EngineError::InvalidTorrent(
                "an info hash must be 40 hexadecimal characters".to_owned(),
            ))
        }
    }

    /// Lowercase hexadecimal form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InfoHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Which networks an engine may use. Every profile is explicit; adapters must
/// not fall back to their own engine defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkProfile {
    /// Loopback only: no DHT, trackers, local discovery, or port mapping. For tests and CI.
    Loopback,
    /// Local network: listens on all interfaces and uses local service
    /// discovery and the torrent's own trackers. No DHT and no port mapping.
    LocalNetwork,
    /// Local network plus the public DHT. No port mapping.
    Internet,
}

impl NetworkProfile {
    /// Stable storage and IPC name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Loopback => "loopback",
            Self::LocalNetwork => "local_network",
            Self::Internet => "internet",
        }
    }

    /// Parse a stable name produced by [`Self::name`].
    #[must_use]
    pub fn from_name(value: &str) -> Option<Self> {
        [Self::Loopback, Self::LocalNetwork, Self::Internet]
            .into_iter()
            .find(|profile| profile.name() == value)
    }
}

/// How peers outside this computer reach an engine. A fixed port applies to
/// [`NetworkProfile::LocalNetwork`] and [`NetworkProfile::Internet`]; router
/// port forwarding applies to [`NetworkProfile::Internet`] only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PeerReachability {
    /// Fixed peer port, or `None` for a new ephemeral port on each start.
    pub port: Option<u16>,
    /// Ask the router to forward the peer port with `UPnP`.
    pub port_forwarding: bool,
}

impl PeerReachability {
    /// Lowest fixed port accepted; lower ports need elevated privileges.
    pub const MIN_FIXED_PORT: u16 = 1024;

    /// Validate a choice: a fixed port must be at least [`Self::MIN_FIXED_PORT`].
    #[must_use]
    pub fn new(port: Option<u16>, port_forwarding: bool) -> Option<Self> {
        port.is_none_or(|port| port >= Self::MIN_FIXED_PORT)
            .then_some(Self {
                port,
                port_forwarding,
            })
    }

    /// The part of this choice a profile honors.
    #[must_use]
    pub const fn for_profile(self, profile: NetworkProfile) -> Self {
        match profile {
            NetworkProfile::Loopback => Self {
                port: None,
                port_forwarding: false,
            },
            NetworkProfile::LocalNetwork => Self {
                port: self.port,
                port_forwarding: false,
            },
            NetworkProfile::Internet => self,
        }
    }

    /// Stable storage form of the port; `"0"` means ephemeral.
    #[must_use]
    pub fn port_setting(self) -> String {
        self.port.unwrap_or(0).to_string()
    }

    /// Stable storage form of the forwarding choice.
    #[must_use]
    pub const fn port_forwarding_setting(self) -> &'static str {
        if self.port_forwarding { "on" } else { "off" }
    }

    /// Read stored settings. A missing or invalid port means an ephemeral
    /// port. Forwarding is on unless the user turned it off, as in common
    /// torrent clients: without it, two peers behind home routers cannot
    /// reach each other. It still applies only under
    /// [`NetworkProfile::Internet`].
    #[must_use]
    pub fn from_settings(port: Option<&str>, port_forwarding: Option<&str>) -> Self {
        let port = port
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|port| *port >= Self::MIN_FIXED_PORT);
        Self {
            port,
            port_forwarding: port_forwarding != Some("off"),
        }
    }
}

/// Seed an immutable, already complete content directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedRequest {
    /// Complete `.torrent` bytes.
    pub torrent: Vec<u8>,
    /// Directory holding exactly the torrent's files. Adapters must only read it.
    pub content_root: PathBuf,
}

/// Where torrent metadata comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorrentSource {
    /// Complete `.torrent` bytes, resolved without the network.
    Metainfo(Vec<u8>),
    /// A `magnet:` URI with a v1 info hash; metadata is fetched from peers.
    Magnet(String),
}

/// Resolve metadata without downloading content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveRequest {
    /// Metadata source.
    pub source: TorrentSource,
    /// Peers to contact directly, in addition to whatever the profile allows.
    pub initial_peers: Vec<SocketAddr>,
}

/// One file of a resolved torrent, by engine file index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentFile {
    /// Engine file index used for selection.
    pub index: usize,
    /// Untrusted `/`-joined path inside the torrent. Callers must validate it.
    pub path: String,
    /// Declared length.
    pub length: u64,
}

/// Metadata of a torrent whose info hash has been verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTorrent {
    /// Torrent identity.
    pub info_hash: InfoHash,
    /// Root name declared by the torrent.
    pub name: String,
    /// Every file in engine order.
    pub files: Vec<TorrentFile>,
    /// Complete `.torrent` bytes for later restore.
    pub metainfo: Vec<u8>,
    /// Peers connected when metadata arrived; a hint for a following fetch.
    /// Empty for metadata resolved offline.
    pub peers: Vec<SocketAddr>,
}

/// Download only selected files into a caller-owned directory. Repeating the
/// request for an active torrent replaces its selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRequest {
    /// Complete `.torrent` bytes from [`ResolvedTorrent::metainfo`].
    pub metainfo: Vec<u8>,
    /// Writable directory owned by the caller; existing verified data is reused.
    pub content_root: PathBuf,
    /// Engine file indices to download.
    pub files: Vec<usize>,
    /// Peers to contact directly.
    pub initial_peers: Vec<SocketAddr>,
}

/// Seekable reader over verified torrent bytes. Engine types stay behind it.
pub trait VerifiedFile: tokio::io::AsyncRead + tokio::io::AsyncSeek + Send + Unpin {}

impl<T: tokio::io::AsyncRead + tokio::io::AsyncSeek + Send + Unpin> VerifiedFile for T {}

/// Engine-reported transfer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferState {
    /// Existing content is being hash-checked before it can be served.
    Checking,
    /// Selected files are still being downloaded.
    Downloading,
    /// Every selected piece is verified; serving peers.
    Seeding,
    /// Checking or serving failed; nothing is served.
    Failed,
    /// Paused by the user: nothing is downloaded or served, and verified
    /// bytes can still be read.
    Paused,
}

/// Engine-local transfer metrics with their source being this process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferStats {
    /// Torrent identity.
    pub info_hash: InfoHash,
    /// Current state.
    pub state: TransferState,
    /// Bytes verified against piece hashes.
    pub verified_bytes: u64,
    /// Total content bytes.
    pub total_bytes: u64,
    /// Bytes uploaded by this engine session.
    pub uploaded_bytes: u64,
    /// Current upload rate.
    pub upload_bytes_per_second: u64,
    /// Bytes downloaded from peers in this session.
    pub downloaded_bytes: u64,
    /// Currently connected peers.
    pub connected_peers: u32,
    /// Verified bytes per engine file index.
    pub file_verified_bytes: Vec<u64>,
    /// Failure description when `state` is [`TransferState::Failed`].
    pub failure: Option<String>,
}

/// Classified engine failure.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum EngineError {
    /// The engine does not support the requested capability.
    #[error("the torrent engine does not support {0}")]
    UnsupportedCapability(&'static str),
    /// Torrent bytes or identifiers are invalid.
    #[error("invalid torrent: {0}")]
    InvalidTorrent(String),
    /// The content directory does not match the torrent.
    #[error("the content does not match the torrent: {0}")]
    ContentMismatch(String),
    /// The torrent is already active in this engine.
    #[error("the torrent is already active")]
    AlreadyActive,
    /// The torrent is not active in this engine.
    #[error("the torrent is not active")]
    NotActive,
    /// The engine failed for another reason.
    #[error("torrent engine failure: {0}")]
    Engine(String),
}

/// Session-wide transfer speed caps; `None` means unlimited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RateLimits {
    /// Download cap in bytes per second.
    pub download_bytes_per_second: Option<std::num::NonZeroU32>,
    /// Upload cap in bytes per second.
    pub upload_bytes_per_second: Option<std::num::NonZeroU32>,
}

impl RateLimits {
    /// Smallest cap a user may set, so a typo cannot stall every transfer.
    pub const MIN_BYTES_PER_SECOND: u32 = 16 * 1024;

    /// Limits from optional KiB/s values; zero or `None` means unlimited.
    /// Returns `None` when a set value is below
    /// [`Self::MIN_BYTES_PER_SECOND`] or does not fit in bytes per second.
    #[must_use]
    pub fn from_kib_per_second(download: Option<u32>, upload: Option<u32>) -> Option<Self> {
        let convert = |value: Option<u32>| -> Option<Option<std::num::NonZeroU32>> {
            match value {
                None | Some(0) => Some(None),
                Some(kib) => {
                    let bytes = kib.checked_mul(1024)?;
                    (bytes >= Self::MIN_BYTES_PER_SECOND).then(|| std::num::NonZeroU32::new(bytes))
                }
            }
        };
        Some(Self {
            download_bytes_per_second: convert(download)?,
            upload_bytes_per_second: convert(upload)?,
        })
    }

    /// Limits from stored settings in KiB/s. Missing, empty, or invalid
    /// values mean unlimited, so a damaged setting never stalls transfers.
    #[must_use]
    pub fn from_settings(download: Option<&str>, upload: Option<&str>) -> Self {
        let parse = |value: Option<&str>| value.and_then(|text| text.parse::<u32>().ok());
        let only =
            |download, upload| Self::from_kib_per_second(download, upload).unwrap_or_default();
        Self {
            download_bytes_per_second: only(parse(download), None).download_bytes_per_second,
            upload_bytes_per_second: only(None, parse(upload)).upload_bytes_per_second,
        }
    }

    /// Settings values in KiB/s; an empty string means unlimited.
    #[must_use]
    pub fn to_settings(self) -> (String, String) {
        let (download, upload) = self.as_kib_per_second();
        let text = |value: Option<u32>| value.map_or_else(String::new, |kib| kib.to_string());
        (text(download), text(upload))
    }

    /// The caps in KiB/s, for settings and display.
    #[must_use]
    pub fn as_kib_per_second(self) -> (Option<u32>, Option<u32>) {
        (
            self.download_bytes_per_second
                .map(|bytes| bytes.get() / 1024),
            self.upload_bytes_per_second.map(|bytes| bytes.get() / 1024),
        )
    }
}

/// Capabilities an application may request from a torrent engine.
pub trait TorrentEngine: Send + Sync {
    /// Start serving a complete, immutable content directory after the engine
    /// has verified every piece. Returns once checking has started.
    fn seed(
        &self,
        request: SeedRequest,
    ) -> impl Future<Output = Result<InfoHash, EngineError>> + Send;

    /// Resolve and verify torrent metadata without downloading any file.
    /// Dropping the future abandons resolution.
    fn resolve(
        &self,
        request: ResolveRequest,
    ) -> impl Future<Output = Result<ResolvedTorrent, EngineError>> + Send;

    /// Download selected files; returns once the transfer is active.
    fn fetch(
        &self,
        request: FetchRequest,
    ) -> impl Future<Output = Result<InfoHash, EngineError>> + Send;

    /// Replace the set of files an active torrent downloads in full. Pieces of
    /// files removed from the set stay verified on disk; streaming still
    /// reads any file on demand.
    ///
    /// Engines without selection return [`EngineError::UnsupportedCapability`].
    fn select_files(
        &self,
        info_hash: &InfoHash,
        files: &[usize],
    ) -> impl Future<Output = Result<(), EngineError>> + Send {
        let _ = (info_hash, files);
        async { Err(EngineError::UnsupportedCapability("file selection")) }
    }

    /// Stop downloading and serving a torrent without removing it. Verified
    /// bytes stay readable. Pausing a paused torrent succeeds.
    fn pause(&self, info_hash: &InfoHash) -> impl Future<Output = Result<(), EngineError>> + Send {
        let _ = info_hash;
        async { Err(EngineError::UnsupportedCapability("pause")) }
    }

    /// Resume a paused torrent. Resuming a running torrent succeeds.
    fn resume(&self, info_hash: &InfoHash) -> impl Future<Output = Result<(), EngineError>> + Send {
        let _ = info_hash;
        async { Err(EngineError::UnsupportedCapability("resume")) }
    }

    /// Apply session-wide speed caps to every current and future transfer.
    ///
    /// # Errors
    /// Engines without rate limiting return [`EngineError::UnsupportedCapability`].
    fn set_rate_limits(&self, limits: RateLimits) -> Result<(), EngineError> {
        let _ = limits;
        Err(EngineError::UnsupportedCapability("rate limits"))
    }

    /// Stop serving a torrent. Content files are never deleted.
    fn stop(&self, info_hash: &InfoHash) -> impl Future<Output = Result<(), EngineError>> + Send;

    /// Open one file of an active torrent for reading, whether or not the
    /// file is selected for download. Reads return only verified bytes and
    /// wait until the pieces they need are verified; the engine prioritizes
    /// pieces ahead of the read position. Dropping the reader releases that
    /// priority, so seeking or abandoning a read cancels obsolete work.
    fn open_file(
        &self,
        info_hash: &InfoHash,
        file_index: usize,
    ) -> impl Future<Output = Result<Box<dyn VerifiedFile>, EngineError>> + Send;

    /// Metrics for one active torrent.
    fn transfer(&self, info_hash: &InfoHash) -> Option<TransferStats>;

    /// Metrics for every active torrent.
    fn transfers(&self) -> Vec<TransferStats>;

    /// Local peer-listening address, when the engine listens.
    fn listen_address(&self) -> Option<SocketAddr>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limits_convert_from_kib_and_reject_tiny_or_huge_caps() {
        let limits = RateLimits::from_kib_per_second(Some(512), None).unwrap();
        assert_eq!(limits.download_bytes_per_second.unwrap().get(), 512 * 1024);
        assert_eq!(limits.upload_bytes_per_second, None);
        assert_eq!(limits.as_kib_per_second(), (Some(512), None));
        assert_eq!(
            RateLimits::from_kib_per_second(Some(0), Some(0)),
            Some(RateLimits::default())
        );
        assert_eq!(RateLimits::from_kib_per_second(Some(8), None), None);
        assert_eq!(RateLimits::from_kib_per_second(None, Some(u32::MAX)), None);
        assert_eq!(limits.to_settings(), ("512".to_owned(), String::new()));
        assert_eq!(RateLimits::from_settings(Some("512"), Some("")), limits);
        assert_eq!(
            RateLimits::from_settings(Some("x"), Some("4")),
            RateLimits::default()
        );
        assert_eq!(
            RateLimits::from_settings(None, Some("64")).as_kib_per_second(),
            (None, Some(64))
        );
    }

    #[test]
    fn info_hashes_are_normalized_and_validated() {
        let hash = InfoHash::parse("ABCDEF0123456789abcdef0123456789ABCDEF01").unwrap();
        assert_eq!(hash.as_str(), "abcdef0123456789abcdef0123456789abcdef01");
        for bad in ["", "abc", &"g".repeat(40), &"a".repeat(41)] {
            assert!(InfoHash::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn network_profile_names_round_trip() {
        for profile in [
            NetworkProfile::Loopback,
            NetworkProfile::LocalNetwork,
            NetworkProfile::Internet,
        ] {
            assert_eq!(NetworkProfile::from_name(profile.name()), Some(profile));
        }
        assert_eq!(NetworkProfile::from_name("LocalNetwork"), None);
    }

    #[test]
    fn peer_reachability_validates_stores_and_follows_the_profile() {
        assert_eq!(PeerReachability::new(Some(1023), false), None);
        let chosen = PeerReachability::new(Some(51413), true).unwrap();
        assert_eq!(
            PeerReachability::from_settings(
                Some(&chosen.port_setting()),
                Some(chosen.port_forwarding_setting())
            ),
            chosen
        );
        let ephemeral = PeerReachability::new(None, false).unwrap();
        assert_eq!(ephemeral.port_setting(), "0");
        assert_eq!(
            PeerReachability::from_settings(Some("0"), Some("off")),
            ephemeral
        );
        for stored in [None, Some("80"), Some("70000"), Some("port")] {
            assert_eq!(
                PeerReachability::from_settings(stored, None),
                PeerReachability {
                    port: None,
                    port_forwarding: true
                },
                "forwarding defaults to on"
            );
        }
        assert!(!PeerReachability::from_settings(None, Some("off")).port_forwarding);

        assert_eq!(chosen.for_profile(NetworkProfile::Internet), chosen);
        assert_eq!(
            chosen.for_profile(NetworkProfile::LocalNetwork),
            PeerReachability {
                port: Some(51413),
                port_forwarding: false
            }
        );
        assert_eq!(
            chosen.for_profile(NetworkProfile::Loopback),
            PeerReachability::default()
        );
    }
}
