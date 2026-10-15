use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use sha1::{Digest, Sha1};
use thiserror::Error;
use url::{Url, form_urlencoded::byte_serialize};

use crate::bencode::{DecodeError, Value, encode};

const MIN_PIECE_LENGTH: u64 = 16 * 1024;
const MAX_PIECE_LENGTH: u64 = 16 * 1024 * 1024;
const TARGET_MAX_PIECES: u64 = 2_048;
const READ_BUFFER_BYTES: usize = 256 * 1024;

/// One file of a multi-file torrent, addressed by a `/`-separated relative path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageFile {
    /// Package-relative path.
    pub path: String,
    /// Exact byte length.
    pub length: u64,
}

/// Explicit, deterministic metainfo options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TorrentOptions {
    /// Torrent root directory name.
    pub name: String,
    /// Tracker tiers; empty for trackerless (DHT/peer exchange) distribution.
    pub tracker_tiers: Vec<Vec<String>>,
    /// HTTP(S) web seeds (BEP 19).
    pub web_seeds: Vec<String>,
    /// Optional free-text comment.
    pub comment: Option<String>,
    /// Optional producer identification.
    pub created_by: Option<String>,
    /// Optional Unix timestamp. Omit for byte-identical rebuilds.
    pub creation_date: Option<i64>,
}

/// Hashing progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashProgress {
    /// Bytes hashed so far.
    pub hashed_bytes: u64,
    /// Bytes in all files.
    pub total_bytes: u64,
}

/// A serialized torrent and the facts needed for a publishing report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltTorrent {
    /// Complete `.torrent` bytes.
    pub bytes: Vec<u8>,
    /// Lowercase hex SHA-1 of the bencoded `info` dictionary.
    pub info_hash: String,
    /// Selected piece length.
    pub piece_length: u64,
    /// Number of pieces.
    pub piece_count: usize,
    /// Sum of file lengths.
    pub total_length: u64,
    /// Magnet link with display name, trackers, and web seeds.
    pub magnet: String,
}

/// Torrent creation or verification failure. Messages carry package-relative paths only.
#[derive(Debug, Error)]
pub enum TorrentError {
    /// The caller cancelled hashing.
    #[error("torrent hashing was cancelled")]
    Cancelled,
    /// An option is invalid.
    #[error("invalid torrent option: {0}")]
    InvalidOptions(&'static str),
    /// No files were supplied.
    #[error("a torrent needs at least one file")]
    Empty,
    /// A package path is unsafe or duplicated.
    #[error("unsafe or duplicate package path `{0}`")]
    InvalidPath(String),
    /// A file is missing, not regular, or has an unexpected length.
    #[error("package file `{path}` does not match its expected length or type")]
    FileMismatch {
        /// Package-relative path.
        path: String,
    },
    /// Torrent bytes could not be decoded.
    #[error("torrent metainfo is invalid: {0}")]
    Decode(#[from] DecodeError),
    /// Decoded metainfo disagrees with the package.
    #[error("torrent verification failed: {0}")]
    Verification(&'static str),
    /// An I/O operation failed.
    #[error("I/O failure while {operation}: {source}")]
    Io {
        /// Safe operation description.
        operation: &'static str,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
}

/// Deterministic piece length: the smallest power of two of at least 16 KiB
/// that yields at most 2,048 pieces, capped at 16 MiB.
#[must_use]
pub fn piece_length_for(total_length: u64) -> u64 {
    let wanted = total_length
        .div_ceil(TARGET_MAX_PIECES)
        .max(MIN_PIECE_LENGTH);
    wanted
        .checked_next_power_of_two()
        .unwrap_or(MAX_PIECE_LENGTH)
        .min(MAX_PIECE_LENGTH)
}

/// Build v1 multi-file metainfo over an explicit file list below `root`.
///
/// Files are hashed in bytewise path order regardless of input order. Every
/// file must be a regular file (not a symbolic link) with exactly the
/// declared length.
///
/// # Errors
/// Returns a classified [`TorrentError`].
pub fn build_torrent(
    root: &Path,
    files: &[PackageFile],
    options: &TorrentOptions,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(HashProgress),
) -> Result<BuiltTorrent, TorrentError> {
    validate_options(options)?;
    let files = sorted_files(files)?;
    let total_length = files
        .iter()
        .try_fold(0_u64, |total, file| total.checked_add(file.length))
        .ok_or(TorrentError::InvalidOptions("total length overflows"))?;
    let piece_length = piece_length_for(total_length);
    let pieces = hash_pieces(
        root,
        &files,
        piece_length,
        total_length,
        cancelled,
        progress,
    )?;
    let piece_count = pieces.len() / 20;

    let mut info = BTreeMap::new();
    info.insert(
        b"files".to_vec(),
        Value::List(
            files
                .iter()
                .map(|file| {
                    let mut entry = BTreeMap::new();
                    entry.insert(b"length".to_vec(), Value::Integer(to_i64(file.length)?));
                    entry.insert(
                        b"path".to_vec(),
                        Value::List(
                            file.path
                                .split('/')
                                .map(|segment| Value::Bytes(segment.as_bytes().to_vec()))
                                .collect(),
                        ),
                    );
                    Ok(Value::Dict(entry))
                })
                .collect::<Result<_, TorrentError>>()?,
        ),
    );
    info.insert(
        b"name".to_vec(),
        Value::Bytes(options.name.as_bytes().to_vec()),
    );
    info.insert(
        b"piece length".to_vec(),
        Value::Integer(to_i64(piece_length)?),
    );
    info.insert(b"pieces".to_vec(), Value::Bytes(pieces));
    let info = Value::Dict(info);
    let info_hash = hex(&Sha1::digest(encode(&info)));

    let mut root_dict = BTreeMap::new();
    if let Some(first) = options.tracker_tiers.first().and_then(|tier| tier.first()) {
        root_dict.insert(b"announce".to_vec(), bytes(first));
        root_dict.insert(
            b"announce-list".to_vec(),
            Value::List(
                options
                    .tracker_tiers
                    .iter()
                    .map(|tier| Value::List(tier.iter().map(|url| bytes(url)).collect()))
                    .collect(),
            ),
        );
    }
    if let Some(comment) = &options.comment {
        root_dict.insert(b"comment".to_vec(), bytes(comment));
    }
    if let Some(created_by) = &options.created_by {
        root_dict.insert(b"created by".to_vec(), bytes(created_by));
    }
    if let Some(date) = options.creation_date {
        root_dict.insert(b"creation date".to_vec(), Value::Integer(date));
    }
    root_dict.insert(b"info".to_vec(), info);
    if !options.web_seeds.is_empty() {
        root_dict.insert(
            b"url-list".to_vec(),
            Value::List(options.web_seeds.iter().map(|url| bytes(url)).collect()),
        );
    }

    Ok(BuiltTorrent {
        bytes: encode(&Value::Dict(root_dict)),
        magnet: magnet(&info_hash, options),
        info_hash,
        piece_length,
        piece_count,
        total_length,
    })
}

pub(crate) fn sorted_files(files: &[PackageFile]) -> Result<Vec<PackageFile>, TorrentError> {
    if files.is_empty() {
        return Err(TorrentError::Empty);
    }
    let mut sorted = files.to_vec();
    sorted.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    for (index, file) in sorted.iter().enumerate() {
        if !safe_relative_path(&file.path) || index > 0 && sorted[index - 1].path == file.path {
            return Err(TorrentError::InvalidPath(file.path.clone()));
        }
    }
    Ok(sorted)
}

pub(crate) fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|segment| {
            let mut components = Path::new(segment).components();
            matches!(components.next(), Some(Component::Normal(_)))
                && components.next().is_none()
                && !segment.contains(['\\', '\0', ':'])
        })
}

pub(crate) fn open_regular(root: &Path, file: &PackageFile) -> Result<File, TorrentError> {
    let mismatch = || TorrentError::FileMismatch {
        path: file.path.clone(),
    };
    let mut path: PathBuf = root.to_path_buf();
    let mut segments = file.path.split('/').peekable();
    while let Some(segment) = segments.next() {
        path.push(segment);
        if segments.peek().is_some() {
            let directory = fs::symlink_metadata(&path).map_err(|_| mismatch())?;
            if !directory.file_type().is_dir() {
                return Err(mismatch());
            }
        }
    }
    let metadata = fs::symlink_metadata(&path).map_err(|_| mismatch())?;
    if !metadata.file_type().is_file() || metadata.len() != file.length {
        return Err(mismatch());
    }
    File::open(&path).map_err(|_| mismatch())
}

fn hash_pieces(
    root: &Path,
    files: &[PackageFile],
    piece_length: u64,
    total_length: u64,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(HashProgress),
) -> Result<Vec<u8>, TorrentError> {
    let piece_capacity = usize::try_from(piece_length)
        .map_err(|_| TorrentError::InvalidOptions("piece length too large"))?;
    let mut pieces = Vec::new();
    let mut piece = Vec::with_capacity(piece_capacity);
    let mut buffer = vec![0_u8; READ_BUFFER_BYTES];
    let mut hashed = 0_u64;
    for file in files {
        let mut input = open_regular(root, file)?;
        let mut remaining = file.length;
        loop {
            if cancelled() {
                return Err(TorrentError::Cancelled);
            }
            let read = input.read(&mut buffer).map_err(|source| TorrentError::Io {
                operation: "reading a package file",
                source,
            })?;
            if read == 0 {
                break;
            }
            let read_u64 = u64::try_from(read).unwrap_or(u64::MAX);
            if read_u64 > remaining {
                return Err(TorrentError::FileMismatch {
                    path: file.path.clone(),
                });
            }
            remaining -= read_u64;
            let mut chunk = &buffer[..read];
            while !chunk.is_empty() {
                let take = chunk.len().min(piece_capacity - piece.len());
                piece.extend_from_slice(&chunk[..take]);
                chunk = &chunk[take..];
                if piece.len() == piece_capacity {
                    pieces.extend_from_slice(&Sha1::digest(&piece));
                    piece.clear();
                }
            }
            hashed += read_u64;
            progress(HashProgress {
                hashed_bytes: hashed,
                total_bytes: total_length,
            });
        }
        if remaining != 0 {
            return Err(TorrentError::FileMismatch {
                path: file.path.clone(),
            });
        }
    }
    if !piece.is_empty() {
        pieces.extend_from_slice(&Sha1::digest(&piece));
    }
    Ok(pieces)
}

fn validate_options(options: &TorrentOptions) -> Result<(), TorrentError> {
    let name = options.name.as_str();
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
    {
        return Err(TorrentError::InvalidOptions(
            "name must be a single safe path segment",
        ));
    }
    if options.tracker_tiers.iter().any(|tier| {
        tier.is_empty()
            || !tier
                .iter()
                .all(|url| has_scheme(url, &["http", "https", "udp"]))
    }) {
        return Err(TorrentError::InvalidOptions(
            "tracker tiers must be non-empty lists of http, https, or udp URLs",
        ));
    }
    if !options
        .web_seeds
        .iter()
        .all(|url| has_scheme(url, &["http", "https"]))
    {
        return Err(TorrentError::InvalidOptions(
            "web seeds must be http or https URLs",
        ));
    }
    Ok(())
}

fn has_scheme(value: &str, schemes: &[&str]) -> bool {
    Url::parse(value).is_ok_and(|url| schemes.contains(&url.scheme()) && url.host_str().is_some())
}

fn magnet(info_hash: &str, options: &TorrentOptions) -> String {
    let encode = |value: &str| byte_serialize(value.as_bytes()).collect::<String>();
    let mut link = format!(
        "magnet:?xt=urn:btih:{info_hash}&dn={}",
        encode(&options.name)
    );
    for tracker in options.tracker_tiers.iter().flatten() {
        let _ = write!(link, "&tr={}", encode(tracker));
    }
    for seed in &options.web_seeds {
        let _ = write!(link, "&ws={}", encode(seed));
    }
    link
}

fn bytes(value: &str) -> Value {
    Value::Bytes(value.as_bytes().to_vec())
}

fn to_i64(value: u64) -> Result<i64, TorrentError> {
    i64::try_from(value).map_err(|_| TorrentError::InvalidOptions("length exceeds i64"))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}
