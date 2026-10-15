//! Reading lesson documents (PDFs) for the in-app viewer.
//!
//! Documents are handed to the UI as verified bytes: local files are hashed
//! against the manifest on every read, torrent files are read only through
//! the engine's verified stream. Neither path, nor anything a document names,
//! leaves the backend.

use std::{fs::File, io::Read, path::PathBuf, time::Duration};

use course_format::ResourceKind;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::AsyncReadExt;
use torrent_port::{InfoHash, TorrentEngine};

use crate::{CourseLibrary, LibraryError, StreamWait, open_lesson_stream};

/// Largest document read into memory for viewing.
pub const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;
/// How long a torrent document may take to arrive.
const TORRENT_READ_LIMIT: Duration = Duration::from_secs(120);

/// Where a viewable document's bytes come from. Backend-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentSource {
    /// A verified local package file.
    Local {
        /// Filesystem location; never serialize it.
        path: PathBuf,
        /// Manifest digest, lowercase hex.
        sha256: String,
        /// Manifest length.
        byte_length: u64,
    },
    /// A file of a torrent course.
    Torrent {
        /// Torrent identity.
        info_hash: String,
        /// File index in the torrent.
        file_index: usize,
        /// Manifest length.
        byte_length: u64,
    },
}

/// Why a document could not be read. Messages never name a path.
#[derive(Debug, Error)]
pub enum DocumentError {
    /// The resource is not a PDF of this course.
    #[error("the resource is not a document of this course")]
    NotADocument,
    /// The document is larger than the viewer reads.
    #[error("the document is too large to open here")]
    TooLarge,
    /// The local file no longer matches the course.
    #[error("the document on disk no longer matches the course")]
    Changed,
    /// The bytes could not be read or did not arrive in time.
    #[error("the document could not be read")]
    Unavailable,
    /// The library could not be read.
    #[error(transparent)]
    Library(#[from] LibraryError),
}

impl CourseLibrary {
    /// Where a PDF resource's bytes come from, or `None` for other resources.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn document_source(
        &self,
        course_id: &str,
        resource_id: &str,
    ) -> Result<Option<DocumentSource>, LibraryError> {
        let Some(manifest) = self.manifest(course_id)? else {
            return Ok(None);
        };
        let Some(resource) = manifest
            .modules
            .iter()
            .flat_map(|module| &module.lessons)
            .flat_map(|lesson| &lesson.resources)
            .find(|resource| resource.id == resource_id && resource.kind == ResourceKind::Pdf)
        else {
            return Ok(None);
        };
        if self.torrent_info_hash(course_id)?.is_some() {
            return Ok(self
                .torrent_resources(course_id)?
                .into_iter()
                .find(|candidate| candidate.resource_id == resource_id)
                .map(|torrent| DocumentSource::Torrent {
                    info_hash: torrent.info_hash,
                    file_index: torrent.file_index,
                    byte_length: torrent.byte_length,
                }));
        }
        Ok(self
            .local_resource(course_id, resource_id)?
            .map(|local| DocumentSource::Local {
                path: local.path,
                sha256: resource.sha256.clone(),
                byte_length: local.byte_length,
            }))
    }
}

/// Read a local document, refusing anything larger than [`MAX_DOCUMENT_BYTES`]
/// or not matching its manifest length and digest.
///
/// # Errors
///
/// Returns [`DocumentError`].
pub fn read_local_document(
    path: &std::path::Path,
    sha256: &str,
    byte_length: u64,
) -> Result<Vec<u8>, DocumentError> {
    if byte_length > MAX_DOCUMENT_BYTES {
        return Err(DocumentError::TooLarge);
    }
    let file = File::open(path).map_err(|_| DocumentError::Unavailable)?;
    let mut bytes = Vec::new();
    file.take(byte_length + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DocumentError::Unavailable)?;
    let digest = Sha256::digest(&bytes);
    let hex = digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        });
    if bytes.len() as u64 != byte_length || hex != sha256 {
        return Err(DocumentError::Changed);
    }
    Ok(bytes)
}

/// Read a torrent document through the engine's verified stream, waiting for
/// missing pieces up to a time limit.
///
/// # Errors
///
/// Returns [`DocumentError`].
pub async fn read_torrent_document<E: TorrentEngine + ?Sized>(
    engine: &E,
    info_hash: &str,
    file_index: usize,
    byte_length: u64,
) -> Result<Vec<u8>, DocumentError> {
    if byte_length > MAX_DOCUMENT_BYTES {
        return Err(DocumentError::TooLarge);
    }
    let info_hash = InfoHash::parse(info_hash).map_err(|_| DocumentError::Unavailable)?;
    let read = async {
        let file = open_lesson_stream(engine, &info_hash, file_index, StreamWait::default())
            .await
            .map_err(|_| DocumentError::Unavailable)?;
        let mut bytes = Vec::new();
        file.take(byte_length)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| DocumentError::Unavailable)?;
        if bytes.len() as u64 == byte_length {
            Ok(bytes)
        } else {
            Err(DocumentError::Unavailable)
        }
    };
    tokio::time::timeout(TORRENT_READ_LIMIT, read)
        .await
        .map_err(|_| DocumentError::Unavailable)?
}
