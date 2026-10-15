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

/// Seed an immutable, already complete content directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedRequest {
    /// Complete `.torrent` bytes.
    pub torrent: Vec<u8>,
    /// Directory holding exactly the torrent's files. Adapters must only read it.
    pub content_root: PathBuf,
}

/// Engine-reported transfer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferState {
    /// Existing content is being hash-checked before it can be served.
    Checking,
    /// All pieces verified; serving peers.
    Seeding,
    /// Checking or serving failed; nothing is served.
    Failed,
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
    /// Currently connected peers.
    pub connected_peers: u32,
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

/// Capabilities an application may request from a torrent engine.
pub trait TorrentEngine: Send + Sync {
    /// Start serving a complete, immutable content directory after the engine
    /// has verified every piece. Returns once checking has started.
    fn seed(
        &self,
        request: SeedRequest,
    ) -> impl Future<Output = Result<InfoHash, EngineError>> + Send;

    /// Stop serving a torrent. Content files are never deleted.
    fn stop(&self, info_hash: &InfoHash) -> impl Future<Output = Result<(), EngineError>> + Send;

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
    fn info_hashes_are_normalized_and_validated() {
        let hash = InfoHash::parse("ABCDEF0123456789abcdef0123456789ABCDEF01").unwrap();
        assert_eq!(hash.as_str(), "abcdef0123456789abcdef0123456789abcdef01");
        for bad in ["", "abc", &"g".repeat(40), &"a".repeat(41)] {
            assert!(InfoHash::parse(bad).is_err(), "{bad}");
        }
    }
}
