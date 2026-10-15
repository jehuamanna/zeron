use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use course_format::parse_and_validate;
use course_publisher::{MANIFEST_FILE_NAME, PackagingCancellation};
use torrent_metainfo::{
    HashProgress, PackageFile, TorrentError, TorrentOptions, build_torrent, verify_torrent,
};

use crate::{DraftServiceError, staging::is_completed_attempt};

const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TORRENT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TREE_ENTRIES: usize = 100_000;
const CREATED_BY: &str = "Zeron Publisher";

/// Tracker tiers and web seeds chosen by the publisher.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DistributionOptions {
    /// Tracker tiers; empty for trackerless distribution.
    pub tracker_tiers: Vec<Vec<String>>,
    /// HTTP(S) web seeds.
    pub web_seeds: Vec<String>,
}

/// Path-free result of generating and verifying a torrent for a staged attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentReceipt {
    /// Staging attempt the torrent describes.
    pub attempt_id: String,
    /// Lowercase hex v1 info hash.
    pub info_hash: String,
    /// Magnet link.
    pub magnet: String,
    /// Deterministically selected piece length.
    pub piece_length: u64,
    /// Number of pieces.
    pub piece_count: usize,
    /// Number of files, including `course.json`.
    pub file_count: usize,
    /// Sum of file lengths.
    pub total_bytes: u64,
}

/// Generate `<attempt>.torrent` beside a completed staging attempt and verify
/// it by re-reading the written bytes and rehashing the package.
///
/// # Errors
/// Rejects unknown or tampered attempts, existing torrents, and any hashing or
/// verification failure. A torrent that fails verification is removed.
pub fn generate_torrent(
    staging_root: &Path,
    attempt_id: &str,
    distribution: &DistributionOptions,
    cancellation: &PackagingCancellation,
    progress: &mut dyn FnMut(HashProgress),
) -> Result<TorrentReceipt, DraftServiceError> {
    let attempt = StagedAttempt::open(staging_root, attempt_id)?;
    let torrent_path = attempt.torrent_path();
    if fs::symlink_metadata(&torrent_path).is_ok() {
        return Err(DraftServiceError::InvalidAttempt(
            "a torrent was already generated",
        ));
    }
    let options = TorrentOptions {
        name: format!("{}-{}", attempt.course_id, attempt.course_version),
        tracker_tiers: distribution.tracker_tiers.clone(),
        web_seeds: distribution.web_seeds.clone(),
        comment: None,
        created_by: Some(CREATED_BY.to_owned()),
        creation_date: None,
    };
    let cancelled = || cancellation.is_cancelled();
    let built = build_torrent(
        &attempt.root,
        &attempt.files,
        &options,
        &cancelled,
        progress,
    )
    .map_err(DraftServiceError::Torrent)?;
    write_atomically(&torrent_path, &built.bytes)?;
    let verified = read_torrent(&torrent_path).and_then(|bytes| {
        verify_torrent(
            &bytes,
            &attempt.root,
            &attempt.files,
            &cancelled,
            &mut |_| {},
        )
        .map_err(DraftServiceError::Torrent)
    });
    match verified {
        Ok(verified) if verified.info_hash == built.info_hash => Ok(TorrentReceipt {
            attempt_id: attempt_id.to_owned(),
            info_hash: built.info_hash,
            magnet: built.magnet,
            piece_length: built.piece_length,
            piece_count: built.piece_count,
            file_count: attempt.files.len(),
            total_bytes: built.total_length,
        }),
        Ok(_) => {
            let _ = fs::remove_file(&torrent_path);
            Err(DraftServiceError::Torrent(TorrentError::Verification(
                "the written torrent does not match the built torrent",
            )))
        }
        Err(error) => {
            let _ = fs::remove_file(&torrent_path);
            Err(error)
        }
    }
}

/// Re-verify an existing `<attempt>.torrent` against its staged package.
///
/// # Errors
/// Returns a verification failure when any byte, file, or table entry differs.
pub fn verify_staged_torrent(
    staging_root: &Path,
    attempt_id: &str,
    cancellation: &PackagingCancellation,
    progress: &mut dyn FnMut(HashProgress),
) -> Result<String, DraftServiceError> {
    let attempt = StagedAttempt::open(staging_root, attempt_id)?;
    let bytes = read_torrent(&attempt.torrent_path())?;
    let cancelled = || cancellation.is_cancelled();
    verify_torrent(&bytes, &attempt.root, &attempt.files, &cancelled, progress)
        .map(|verified| verified.info_hash)
        .map_err(DraftServiceError::Torrent)
}

struct StagedAttempt {
    staging_root: PathBuf,
    attempt_id: String,
    root: PathBuf,
    course_id: String,
    course_version: String,
    files: Vec<PackageFile>,
}

impl StagedAttempt {
    fn open(staging_root: &Path, attempt_id: &str) -> Result<Self, DraftServiceError> {
        if !is_completed_attempt(attempt_id) {
            return Err(DraftServiceError::InvalidAttempt("unknown staging attempt"));
        }
        let root = staging_root.join(attempt_id);
        let metadata = fs::symlink_metadata(&root)
            .map_err(|_| DraftServiceError::InvalidAttempt("unknown staging attempt"))?;
        if !metadata.file_type().is_dir() {
            return Err(DraftServiceError::InvalidAttempt("unknown staging attempt"));
        }
        let manifest_bytes = read_bounded(&root.join(MANIFEST_FILE_NAME), MAX_MANIFEST_BYTES)
            .map_err(|_| {
                DraftServiceError::InvalidAttempt("the staged course.json is unreadable")
            })?;
        let manifest = parse_and_validate(&manifest_bytes)
            .map_err(|_| DraftServiceError::InvalidAttempt("the staged course.json is invalid"))?;
        let mut files: Vec<PackageFile> = manifest
            .modules
            .iter()
            .flat_map(|module| &module.lessons)
            .flat_map(|lesson| &lesson.resources)
            .map(|resource| PackageFile {
                path: resource.path.clone(),
                length: resource.byte_length,
            })
            .collect();
        files.push(PackageFile {
            path: MANIFEST_FILE_NAME.to_owned(),
            length: manifest_bytes.len() as u64,
        });
        let expected: BTreeSet<&str> = files.iter().map(|file| file.path.as_str()).collect();
        if staged_tree(&root)? != expected.iter().map(|path| (*path).to_owned()).collect() {
            return Err(DraftServiceError::InvalidAttempt(
                "the staged files differ from course.json",
            ));
        }
        Ok(Self {
            staging_root: staging_root.to_path_buf(),
            attempt_id: attempt_id.to_owned(),
            root,
            course_id: manifest.course_id,
            course_version: manifest.version,
            files,
        })
    }

    fn torrent_path(&self) -> PathBuf {
        self.staging_root
            .join(format!("{}.torrent", self.attempt_id))
    }
}

fn staged_tree(root: &Path) -> Result<BTreeSet<String>, DraftServiceError> {
    let tampered =
        || DraftServiceError::InvalidAttempt("the staged package contains unexpected entries");
    let mut files = BTreeSet::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    let mut visited = 0;
    while let Some((directory, prefix)) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|_| tampered())?;
        for entry in entries {
            visited += 1;
            if visited > MAX_TREE_ENTRIES {
                return Err(tampered());
            }
            let entry = entry.map_err(|_| tampered())?;
            let name = entry.file_name().into_string().map_err(|_| tampered())?;
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let file_type = entry.file_type().map_err(|_| tampered())?;
            if file_type.is_dir() {
                pending.push((entry.path(), relative));
            } else if file_type.is_file() {
                files.insert(relative);
            } else {
                return Err(tampered());
            }
        }
    }
    Ok(files)
}

fn read_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a bounded regular file",
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file grew beyond its limit",
        ));
    }
    Ok(bytes)
}

fn read_torrent(path: &Path) -> Result<Vec<u8>, DraftServiceError> {
    read_bounded(path, MAX_TORRENT_BYTES)
        .map_err(|_| DraftServiceError::InvalidAttempt("the torrent file is missing or unreadable"))
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), DraftServiceError> {
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let io_error =
        |operation, source| DraftServiceError::Torrent(TorrentError::Io { operation, source });
    let result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .map_err(|source| io_error("writing the torrent file", source))
        .and_then(|()| {
            fs::rename(&partial, path)
                .map_err(|source| io_error("completing the torrent file", source))
        });
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}
