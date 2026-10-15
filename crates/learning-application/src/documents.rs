//! Reading lesson documents (PDFs and transcripts) for the in-app viewers.
//!
//! Documents are handed to the UI as verified bytes: local files are hashed
//! against the manifest on every read, torrent files are read only through
//! the engine's verified stream. Neither path, nor anything a document names,
//! leaves the backend.

use std::{fs::File, io::Read, path::PathBuf, time::Duration};

use course_format::{ResourceKind, ResourceRelationKind};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::AsyncReadExt;
use torrent_port::{InfoHash, TorrentEngine};

use crate::{CourseLibrary, LibraryError, StreamWait, open_lesson_stream};

/// Largest document read into memory for viewing.
pub const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;
/// Largest Markdown or text resource read for the reader.
pub const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
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
        self.source_of_kind(course_id, resource_id, &[ResourceKind::Pdf])
    }

    /// Where a Markdown or plain-text resource's bytes come from, with whether
    /// it is Markdown, or `None` for other resources.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn text_source(
        &self,
        course_id: &str,
        resource_id: &str,
    ) -> Result<Option<(bool, DocumentSource)>, LibraryError> {
        let Some(manifest) = self.manifest(course_id)? else {
            return Ok(None);
        };
        let markdown = manifest
            .modules
            .iter()
            .flat_map(|module| &module.lessons)
            .flat_map(|lesson| &lesson.resources)
            .find(|resource| resource.id == resource_id)
            .is_some_and(|resource| resource.kind == ResourceKind::Markdown);
        Ok(self
            .source_of_kind(
                course_id,
                resource_id,
                &[ResourceKind::Markdown, ResourceKind::Text],
            )?
            .map(|source| (markdown, source)))
    }

    /// The transcript of a video or audio resource: the resource it names as
    /// its transcript, else as its subtitle track, else the first transcript
    /// and then subtitle resource of the same lesson. Returns the transcript's
    /// resource ID, language, and source.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn transcript_source(
        &self,
        course_id: &str,
        media_resource_id: &str,
    ) -> Result<Option<(String, Option<String>, DocumentSource)>, LibraryError> {
        let Some(manifest) = self.manifest(course_id)? else {
            return Ok(None);
        };
        let Some(lesson) = manifest
            .modules
            .iter()
            .flat_map(|module| &module.lessons)
            .find(|lesson| {
                lesson
                    .resources
                    .iter()
                    .any(|resource| resource.id == media_resource_id)
            })
        else {
            return Ok(None);
        };
        let media = lesson
            .resources
            .iter()
            .find(|resource| resource.id == media_resource_id)
            .filter(|resource| matches!(resource.kind, ResourceKind::Video | ResourceKind::Audio));
        let Some(media) = media else {
            return Ok(None);
        };
        let timed = |id: &str| {
            lesson.resources.iter().find(|resource| {
                resource.id == id
                    && matches!(
                        resource.kind,
                        ResourceKind::Transcript | ResourceKind::Subtitle
                    )
            })
        };
        let related = |kind: ResourceRelationKind| {
            media
                .relations
                .iter()
                .filter(|relation| relation.kind == kind)
                .find_map(|relation| timed(&relation.resource_id))
        };
        let of_kind = |kind: ResourceKind| {
            lesson
                .resources
                .iter()
                .find(|resource| resource.kind == kind)
        };
        let Some(transcript) = related(ResourceRelationKind::Transcript)
            .or_else(|| related(ResourceRelationKind::Subtitle))
            .or_else(|| of_kind(ResourceKind::Transcript))
            .or_else(|| of_kind(ResourceKind::Subtitle))
        else {
            return Ok(None);
        };
        let kinds = [ResourceKind::Transcript, ResourceKind::Subtitle];
        Ok(self
            .source_of_kind(course_id, &transcript.id, &kinds)?
            .map(|source| (transcript.id.clone(), transcript.language.clone(), source)))
    }

    fn source_of_kind(
        &self,
        course_id: &str,
        resource_id: &str,
        kinds: &[ResourceKind],
    ) -> Result<Option<DocumentSource>, LibraryError> {
        let Some(manifest) = self.manifest(course_id)? else {
            return Ok(None);
        };
        let Some(resource) = manifest
            .modules
            .iter()
            .flat_map(|module| &module.lessons)
            .flat_map(|lesson| &lesson.resources)
            .find(|resource| resource.id == resource_id && kinds.contains(&resource.kind))
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

impl DocumentSource {
    /// Manifest length of the document.
    #[must_use]
    pub const fn byte_length(&self) -> u64 {
        match self {
            Self::Local { byte_length, .. } | Self::Torrent { byte_length, .. } => *byte_length,
        }
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
