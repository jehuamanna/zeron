//! What the seed server keeps seeding, stored in its data directory so it
//! survives restarts. Management commands edit it; the running server
//! notices changes and follows them.
//!
//! Layout below the data directory:
//!
//! - `seeds.json`: the list, replaced atomically on every change;
//! - `metainfo/<info hash>.torrent`: metadata once known;
//! - `content/<info hash>/`: downloads of followed torrents;
//! - `engine/`: engine state, which holds nothing persistent.

use std::{
    fs, io,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Longest magnet link accepted.
pub const MAX_MAGNET_BYTES: usize = 8 * 1024;
/// Largest `.torrent` file accepted.
pub const MAX_TORRENT_BYTES: u64 = 16 * 1024 * 1024;
const LIST_FILE: &str = "seeds.json";

/// Where a seeded torrent's content comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Source {
    /// Join the torrent's swarm, download everything from whoever has it,
    /// then keep serving it. This makes the server a reachable hub for a
    /// publisher whose router cannot accept connections.
    Follow {
        /// The release's magnet link.
        magnet: String,
        /// Peers to contact directly, such as the publisher, in addition to
        /// the DHT and the magnet's trackers.
        #[serde(default)]
        peers: Vec<SocketAddr>,
    },
    /// Serve a complete release copied to this server, read-only.
    Release {
        /// Directory holding exactly the torrent's files.
        content_root: PathBuf,
    },
}

/// One torrent to seed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Lowercase hexadecimal v1 info hash.
    pub info_hash: String,
    /// Where its content comes from.
    pub source: Source,
    /// When it was added, in Unix seconds.
    pub added_at_unix_seconds: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct List {
    version: u32,
    torrents: Vec<Entry>,
}

/// Why a registry change was refused.
#[derive(Debug, Error)]
pub enum RegistryError {
    /// The magnet link is not a v1 magnet with a 40-hex or 32-base32 hash.
    #[error("not a magnet link with a v1 info hash (urn:btih)")]
    InvalidMagnet,
    /// The torrent is already listed.
    #[error("{0} is already being seeded")]
    AlreadyListed(String),
    /// The torrent file or release directory is unusable.
    #[error("{0}")]
    InvalidRelease(String),
    /// A file operation failed.
    #[error("{step}: {source}")]
    Io {
        /// What was being done.
        step: &'static str,
        /// The underlying failure.
        source: io::Error,
    },
    /// The list is unreadable.
    #[error("the seed list is unreadable: {0}")]
    Corrupt(#[from] serde_json::Error),
}

fn io(step: &'static str) -> impl FnOnce(io::Error) -> RegistryError {
    move |source| RegistryError::Io { step, source }
}

/// The seed list and its files below one data directory.
#[derive(Debug, Clone)]
pub struct Registry {
    data: PathBuf,
}

impl Registry {
    /// Open, creating the directory layout when missing.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::Io`] when the directories cannot be created.
    pub fn open(data: &Path) -> Result<Self, RegistryError> {
        for directory in ["metainfo", "content", "engine"] {
            fs::create_dir_all(data.join(directory)).map_err(io("creating the data directory"))?;
        }
        Ok(Self {
            data: data.to_path_buf(),
        })
    }

    /// Engine state directory.
    #[must_use]
    pub fn engine_directory(&self) -> PathBuf {
        self.data.join("engine")
    }

    /// Download directory of a followed torrent. `info_hash` is validated
    /// hex, so it can never leave `content/`.
    #[must_use]
    pub fn content_directory(&self, info_hash: &str) -> PathBuf {
        self.data.join("content").join(info_hash)
    }

    /// Every listed torrent, in the order added.
    ///
    /// # Errors
    ///
    /// An unreadable list.
    pub fn entries(&self) -> Result<Vec<Entry>, RegistryError> {
        Ok(self.read()?.torrents)
    }

    /// When the list last changed, for noticing edits made while serving.
    #[must_use]
    pub fn modified(&self) -> Option<SystemTime> {
        fs::metadata(self.data.join(LIST_FILE))
            .and_then(|metadata| metadata.modified())
            .ok()
    }

    /// Follow a magnet link.
    ///
    /// # Errors
    ///
    /// An invalid magnet, a torrent already listed, or a failed write.
    pub fn add_magnet(&self, magnet: &str, peers: Vec<SocketAddr>) -> Result<Entry, RegistryError> {
        let magnet = magnet.trim();
        if magnet.len() > MAX_MAGNET_BYTES {
            return Err(RegistryError::InvalidMagnet);
        }
        let info_hash = magnet_info_hash(magnet).ok_or(RegistryError::InvalidMagnet)?;
        self.add(Entry {
            info_hash,
            source: Source::Follow {
                magnet: magnet.to_owned(),
                peers,
            },
            added_at_unix_seconds: now(),
        })
    }

    /// Seed a release whose `.torrent` is `torrent` and whose files sit in
    /// `content_root`. `info_hash` must be the torrent's, as the engine read it.
    ///
    /// # Errors
    ///
    /// A torrent already listed, an unusable content directory, or a failed write.
    pub fn add_release(
        &self,
        info_hash: &str,
        torrent: &[u8],
        content_root: &Path,
    ) -> Result<Entry, RegistryError> {
        let content_root = fs::canonicalize(content_root)
            .ok()
            .filter(|root| root.is_dir())
            .ok_or_else(|| {
                RegistryError::InvalidRelease("the release's files are not a directory".to_owned())
            })?;
        let info_hash = hex_hash(info_hash).ok_or(RegistryError::InvalidMagnet)?;
        let entry = self.add(Entry {
            info_hash: info_hash.clone(),
            source: Source::Release { content_root },
            added_at_unix_seconds: now(),
        })?;
        self.save_metainfo(&info_hash, torrent)?;
        Ok(entry)
    }

    /// Stop listing a torrent. Its downloaded content stays on disk.
    /// Returns whether it was listed.
    ///
    /// # Errors
    ///
    /// A failed write.
    pub fn remove(&self, info_hash: &str) -> Result<bool, RegistryError> {
        let mut list = self.read()?;
        let before = list.torrents.len();
        let wanted = info_hash.to_ascii_lowercase();
        list.torrents.retain(|entry| entry.info_hash != wanted);
        if list.torrents.len() == before {
            return Ok(false);
        }
        self.write(&list)?;
        Ok(true)
    }

    /// Stored metadata of a torrent, once known.
    #[must_use]
    pub fn metainfo(&self, info_hash: &str) -> Option<Vec<u8>> {
        let hash = hex_hash(info_hash)?;
        fs::read(self.data.join("metainfo").join(format!("{hash}.torrent"))).ok()
    }

    /// Keep a torrent's metadata so a restart needs no peer to resume.
    ///
    /// # Errors
    ///
    /// A failed write.
    pub fn save_metainfo(&self, info_hash: &str, bytes: &[u8]) -> Result<(), RegistryError> {
        let hash = hex_hash(info_hash).ok_or(RegistryError::InvalidMagnet)?;
        write_atomically(
            &self.data.join("metainfo").join(format!("{hash}.torrent")),
            bytes,
        )
    }

    fn add(&self, entry: Entry) -> Result<Entry, RegistryError> {
        let mut list = self.read()?;
        if list
            .torrents
            .iter()
            .any(|listed| listed.info_hash == entry.info_hash)
        {
            return Err(RegistryError::AlreadyListed(entry.info_hash));
        }
        list.torrents.push(entry.clone());
        self.write(&list)?;
        Ok(entry)
    }

    fn read(&self) -> Result<List, RegistryError> {
        match fs::read(self.data.join(LIST_FILE)) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(List {
                version: 1,
                torrents: Vec::new(),
            }),
            Err(error) => Err(io("reading the seed list")(error)),
        }
    }

    fn write(&self, list: &List) -> Result<(), RegistryError> {
        let bytes = serde_json::to_vec_pretty(&List {
            version: 1,
            torrents: list.torrents.clone(),
        })?;
        write_atomically(&self.data.join(LIST_FILE), &bytes)
    }
}

/// Write to a temporary file, flush it, and rename it into place, so a
/// reader never sees half a file.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), RegistryError> {
    use io::Write as _;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let mut file = fs::File::create(&temporary).map_err(io("writing the seed list"))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(io("writing the seed list"))?;
    fs::rename(&temporary, path).map_err(io("writing the seed list"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn hex_hash(value: &str) -> Option<String> {
    (value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

/// The v1 info hash of a magnet link, as lowercase hex. Accepts the 40-hex
/// and the 32-character base32 forms of `xt=urn:btih:`.
#[must_use]
pub fn magnet_info_hash(magnet: &str) -> Option<String> {
    let query = magnet
        .get(..8)
        .filter(|scheme| scheme.eq_ignore_ascii_case("magnet:?"))
        .map(|_| &magnet[8..])?;
    query.split('&').find_map(|parameter| {
        let (name, value) = parameter.split_once('=')?;
        if !name.eq_ignore_ascii_case("xt") {
            return None;
        }
        let hash = value
            .get(..9)
            .filter(|urn| urn.eq_ignore_ascii_case("urn:btih:"))
            .map(|_| &value[9..])?;
        hex_hash(hash).or_else(|| base32_to_hex(hash))
    })
}

fn base32_to_hex(value: &str) -> Option<String> {
    use std::fmt::Write as _;
    if value.len() != 32 {
        return None;
    }
    let mut bits: u64 = 0;
    let mut count = 0;
    let mut bytes = Vec::with_capacity(20);
    for character in value.bytes() {
        let digit = match character.to_ascii_uppercase() {
            letter @ b'A'..=b'Z' => letter - b'A',
            digit @ b'2'..=b'7' => digit - b'2' + 26,
            _ => return None,
        };
        bits = (bits << 5) | u64::from(digit);
        count += 5;
        if count >= 8 {
            count -= 8;
            bytes.push(u8::try_from((bits >> count) & 0xFF).ok()?);
        }
    }
    Some(
        bytes
            .iter()
            .fold(String::with_capacity(40), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnets_yield_their_v1_hash_in_either_encoding() {
        let hex = "0848f0deefc748ab73d86e407f905d8936ff8bc5";
        assert_eq!(
            magnet_info_hash(&format!(
                "magnet:?xt=urn:btih:{}&dn=course",
                hex.to_uppercase()
            ))
            .as_deref(),
            Some(hex)
        );
        // The same hash in base32.
        assert_eq!(
            magnet_info_hash("MAGNET:?dn=x&xt=urn:btih:BBEPBXXPY5EKW46YNZAH7EC5RE3P7C6F")
                .as_deref(),
            Some(hex)
        );
        for invalid in [
            "magnet:?dn=only-a-name",
            "magnet:?xt=urn:btmh:1220abcd",
            "magnet:?xt=urn:btih:xyz",
            "http://example.com/?xt=urn:btih:0848f0deefc748ab73d86e407f905d8936ff8bc5",
            "",
        ] {
            assert_eq!(magnet_info_hash(invalid), None, "{invalid}");
        }
    }
}
