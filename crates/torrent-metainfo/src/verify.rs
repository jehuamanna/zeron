use std::{
    collections::BTreeMap,
    io::{self, Read},
    path::Path,
};

use sha1::{Digest, Sha1};

use crate::{
    bencode::{Value, decode_dict_with_spans, encode},
    builder::{HashProgress, PackageFile, TorrentError, hex, open_regular, sorted_files},
};

/// Facts established by independently re-reading torrent bytes and package files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedTorrent {
    /// Lowercase hex SHA-1 of the raw `info` bytes.
    pub info_hash: String,
    /// Torrent root name.
    pub name: String,
    /// Declared piece length.
    pub piece_length: u64,
    /// Number of verified pieces.
    pub piece_count: usize,
    /// Sum of file lengths.
    pub total_length: u64,
}

/// Lowercase hex v1 info hash: the SHA-1 of the raw `info` bytes as they
/// appear in `torrent`, whether or not they are canonically encoded.
///
/// # Errors
/// Returns a decode error, or [`TorrentError::Verification`] without an `info` dictionary.
pub fn info_hash(torrent: &[u8]) -> Result<String, TorrentError> {
    let top = decode_dict_with_spans(torrent)?;
    let (_, span) = top
        .get(b"info".as_slice())
        .ok_or(TorrentError::Verification("missing info dictionary"))?;
    Ok(hex(&Sha1::digest(&torrent[span.clone()])))
}

/// Files in the order the torrent declares them, which is the order engines
/// index them in. Paths are untrusted; callers must validate them.
///
/// # Errors
/// Returns a decode error or [`TorrentError::Verification`] for a malformed file list.
pub fn file_table(torrent: &[u8]) -> Result<Vec<PackageFile>, TorrentError> {
    let top = decode_dict_with_spans(torrent)?;
    let Some((Value::Dict(info), _)) = top.get(b"info".as_slice()) else {
        return Err(TorrentError::Verification("missing info dictionary"));
    };
    declared_files(info)
}

/// Decode `torrent` strictly and check that it describes exactly `expected`
/// files, without reading package bytes.
///
/// # Errors
/// Returns [`TorrentError::Verification`] for any structural disagreement.
pub fn inspect_torrent(
    torrent: &[u8],
    expected: &[PackageFile],
) -> Result<VerifiedTorrent, TorrentError> {
    structure(torrent, expected).map(|structure| structure.summary)
}

/// Decode `torrent` strictly, check that it describes exactly `expected`
/// files, and rehash every piece from the files below `root`.
///
/// # Errors
/// Returns [`TorrentError::Verification`] for any disagreement.
pub fn verify_torrent(
    torrent: &[u8],
    root: &Path,
    expected: &[PackageFile],
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(HashProgress),
) -> Result<VerifiedTorrent, TorrentError> {
    let Structure {
        summary,
        pieces,
        files,
    } = structure(torrent, expected)?;
    let mut stream = PackageStream::new(root, &files);
    let piece_capacity = usize::try_from(summary.piece_length)
        .map_err(|_| TorrentError::Verification("piece length too large"))?;
    let mut piece = vec![0_u8; piece_capacity];
    let mut verified = 0_u64;
    for expected_digest in pieces.chunks_exact(20) {
        if cancelled() {
            return Err(TorrentError::Cancelled);
        }
        let filled = stream.fill(&mut piece)?;
        if Sha1::digest(&piece[..filled]).as_slice() != expected_digest {
            return Err(TorrentError::Verification(
                "a piece hash does not match the package bytes",
            ));
        }
        verified += u64::try_from(filled).unwrap_or(u64::MAX);
        progress(HashProgress {
            hashed_bytes: verified,
            total_bytes: summary.total_length,
        });
    }
    if stream.fill(&mut piece[..1.min(piece_capacity)])? != 0 || verified != summary.total_length {
        return Err(TorrentError::Verification(
            "package bytes do not end with the last piece",
        ));
    }
    Ok(summary)
}

struct Structure {
    summary: VerifiedTorrent,
    pieces: Vec<u8>,
    files: Vec<PackageFile>,
}

fn structure(torrent: &[u8], expected: &[PackageFile]) -> Result<Structure, TorrentError> {
    let top = decode_dict_with_spans(torrent)?;
    let (info, span) = top
        .get(b"info".as_slice())
        .ok_or(TorrentError::Verification("missing info dictionary"))?;
    if encode(info) != torrent[span.clone()] {
        return Err(TorrentError::Verification(
            "info dictionary is not canonical",
        ));
    }
    let info_hash = hex(&Sha1::digest(&torrent[span.clone()]));
    let Value::Dict(info) = info else {
        return Err(TorrentError::Verification("info is not a dictionary"));
    };
    if info.contains_key(b"length".as_slice()) {
        return Err(TorrentError::Verification(
            "single-file torrents are not expected",
        ));
    }
    let name = text(info.get(b"name".as_slice()))
        .ok_or(TorrentError::Verification("name is missing or not UTF-8"))?;
    let piece_length = match info.get(b"piece length".as_slice()) {
        Some(Value::Integer(length)) if *length > 0 => u64::try_from(*length)
            .map_err(|_| TorrentError::Verification("invalid piece length"))?,
        _ => return Err(TorrentError::Verification("invalid piece length")),
    };
    let Some(Value::Bytes(pieces)) = info.get(b"pieces".as_slice()) else {
        return Err(TorrentError::Verification("pieces are missing"));
    };
    if pieces.len() % 20 != 0 {
        return Err(TorrentError::Verification(
            "pieces are not whole SHA-1 digests",
        ));
    }
    let files = declared_files(info)?;
    if files != sorted_files(expected)? {
        return Err(TorrentError::Verification(
            "file table does not match the package",
        ));
    }
    let total_length = files
        .iter()
        .try_fold(0_u64, |total, file| total.checked_add(file.length))
        .ok_or(TorrentError::Verification("total length overflows"))?;
    let piece_count = usize::try_from(total_length.div_ceil(piece_length))
        .map_err(|_| TorrentError::Verification("too many pieces"))?;
    if piece_count * 20 != pieces.len() {
        return Err(TorrentError::Verification(
            "piece count does not match total length",
        ));
    }
    Ok(Structure {
        summary: VerifiedTorrent {
            info_hash,
            name,
            piece_length,
            piece_count,
            total_length,
        },
        pieces: pieces.clone(),
        files,
    })
}

fn declared_files(info: &BTreeMap<Vec<u8>, Value>) -> Result<Vec<PackageFile>, TorrentError> {
    let Some(Value::List(entries)) = info.get(b"files".as_slice()) else {
        return Err(TorrentError::Verification("file list is missing"));
    };
    entries
        .iter()
        .map(|entry| {
            let Value::Dict(entry) = entry else {
                return Err(TorrentError::Verification("file entry is not a dictionary"));
            };
            let length = match entry.get(b"length".as_slice()) {
                Some(Value::Integer(length)) => u64::try_from(*length)
                    .map_err(|_| TorrentError::Verification("negative file length"))?,
                _ => return Err(TorrentError::Verification("file length is missing")),
            };
            let Some(Value::List(segments)) = entry.get(b"path".as_slice()) else {
                return Err(TorrentError::Verification("file path is missing"));
            };
            let segments = segments
                .iter()
                .map(|segment| text(Some(segment)))
                .collect::<Option<Vec<_>>>()
                .filter(|segments| !segments.is_empty())
                .ok_or(TorrentError::Verification("file path is not UTF-8"))?;
            Ok(PackageFile {
                path: segments.join("/"),
                length,
            })
        })
        .collect()
}

fn text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::Bytes(bytes)) => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    }
}

/// Reads the package files back to back, independent of the builder's loop.
struct PackageStream<'a> {
    root: &'a Path,
    files: &'a [PackageFile],
    next: usize,
    current: Option<io::Take<std::fs::File>>,
}

impl<'a> PackageStream<'a> {
    fn new(root: &'a Path, files: &'a [PackageFile]) -> Self {
        Self {
            root,
            files,
            next: 0,
            current: None,
        }
    }

    fn fill(&mut self, buffer: &mut [u8]) -> Result<usize, TorrentError> {
        let mut filled = 0;
        while filled < buffer.len() {
            if self.current.is_none() {
                let Some(file) = self.files.get(self.next) else {
                    break;
                };
                self.next += 1;
                self.current = Some(open_regular(self.root, file)?.take(file.length));
            }
            let reader = self
                .current
                .as_mut()
                .ok_or(TorrentError::Verification("no reader"))?;
            let read = reader
                .read(&mut buffer[filled..])
                .map_err(|source| TorrentError::Io {
                    operation: "re-reading a package file",
                    source,
                })?;
            if read == 0 {
                if reader.limit() != 0 {
                    return Err(TorrentError::Verification(
                        "a package file is shorter than declared",
                    ));
                }
                self.current = None;
            }
            filled += read;
        }
        Ok(filled)
    }
}
