use std::{
    fmt::Write as _,
    fs::{self, File, Metadata},
    io::{self, BufReader, Read},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

use course_format::{CourseManifest, ManifestError, parse_and_validate};
use sha2::{Digest, Sha256};
use thiserror::Error;

const MANIFEST_FILE_NAME: &str = "course.json";
const HASH_BUFFER_BYTES: usize = 1024 * 1024;

/// Resource limits applied before and during local package verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportLimits {
    /// Maximum accepted `course.json` size.
    pub max_manifest_bytes: u64,
    /// Maximum number of resources declared by one course.
    pub max_resource_count: usize,
    /// Maximum sum of declared resource lengths.
    pub max_total_resource_bytes: u64,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_manifest_bytes: 4 * 1024 * 1024,
            max_resource_count: 10_000,
            max_total_resource_bytes: 4 * 1024 * 1024 * 1024 * 1024,
        }
    }
}

/// A clonable cooperative cancellation signal for blocking import work.
#[derive(Debug, Clone, Default)]
pub struct CancellationFlag(Arc<AtomicBool>);

impl CancellationFlag {
    /// Request cancellation. The importer checks between files and read chunks.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn check(&self) -> Result<(), ImportError> {
        if self.0.load(Ordering::Acquire) {
            Err(ImportError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// A successfully validated local course package.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedCourse {
    /// Validated manifest. It contains package-relative paths only.
    pub manifest: CourseManifest,
    /// Verification evidence for every declared resource.
    pub resources: Vec<VerifiedResource>,
    /// Sum of verified resource lengths.
    pub total_resource_bytes: u64,
    /// Lowercase hex SHA-256 of the exact `course.json` bytes.
    pub manifest_sha256: String,
    /// The bytes of `zeron-signature.json`, when the package has one. They
    /// are untrusted until verified against the manifest.
    pub signature: Option<Vec<u8>>,
}

/// Verification evidence that is safe to expose without an OS path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedResource {
    /// Stable manifest resource ID.
    pub resource_id: String,
    /// Safe package-relative path copied from the validated manifest.
    pub package_path: String,
    /// Verified byte length.
    pub byte_length: u64,
    /// Verified lowercase SHA-256 digest.
    pub sha256: String,
}

/// A classified local import failure.
#[derive(Debug, Error)]
pub enum ImportError {
    /// The caller cancelled the operation.
    #[error("course import was cancelled")]
    Cancelled,
    /// The selected package root is invalid.
    #[error("invalid course root: {0}")]
    InvalidRoot(String),
    /// The manifest cannot be safely read.
    #[error("invalid course manifest file: {0}")]
    InvalidManifestFile(String),
    /// The manifest exceeds the configured byte limit.
    #[error("course manifest is {actual} bytes; limit is {limit} bytes")]
    ManifestTooLarge {
        /// Observed size.
        actual: u64,
        /// Configured maximum.
        limit: u64,
    },
    /// The manifest content is invalid.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// A configured package resource limit was exceeded.
    #[error("course package exceeds limit: {0}")]
    LimitExceeded(String),
    /// A declared resource cannot be safely resolved.
    #[error("resource `{resource_id}` at `{package_path}` is unsafe: {reason}")]
    UnsafeResource {
        /// Stable resource ID.
        resource_id: String,
        /// Manifest-relative path.
        package_path: String,
        /// Rejection reason.
        reason: String,
    },
    /// A declared resource is missing or not a regular file.
    #[error("resource `{resource_id}` at `{package_path}` is missing or not a regular file")]
    MissingResource {
        /// Stable resource ID.
        resource_id: String,
        /// Manifest-relative path.
        package_path: String,
    },
    /// Actual bytes do not match the manifest.
    #[error("resource `{resource_id}` {field} mismatch: expected {expected}, found {actual}")]
    ResourceMismatch {
        /// Stable resource ID.
        resource_id: String,
        /// Mismatched field.
        field: &'static str,
        /// Declared value.
        expected: String,
        /// Observed value.
        actual: String,
    },
    /// A resource changed while it was being verified.
    #[error("resource `{resource_id}` changed during verification")]
    SourceChanged {
        /// Stable resource ID.
        resource_id: String,
    },
    /// An I/O operation failed without exposing an absolute path.
    #[error("I/O failure while {operation}: {source}")]
    Io {
        /// Safe operation description.
        operation: &'static str,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
}

/// Import and verify a local course directory without mutating it.
///
/// The returned value contains no absolute filesystem path. This function is
/// blocking by design; async composition roots must run it on a bounded
/// blocking executor and retain the [`CancellationFlag`].
///
/// # Errors
///
/// Returns a classified [`ImportError`] when root, manifest, limits, resource
/// containment, metadata, or content verification fails.
pub fn import_local_course(
    root: &Path,
    limits: ImportLimits,
    cancellation: &CancellationFlag,
) -> Result<ImportedCourse, ImportError> {
    cancellation.check()?;
    let root_metadata = fs::symlink_metadata(root).map_err(|source| ImportError::Io {
        operation: "reading the course root",
        source,
    })?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(ImportError::InvalidRoot(
            "must be a real directory, not a file or symbolic link".to_owned(),
        ));
    }
    let canonical_root = fs::canonicalize(root).map_err(|source| ImportError::Io {
        operation: "resolving the course root",
        source,
    })?;

    let manifest_bytes = read_manifest(&canonical_root, limits, cancellation)?;
    let manifest = parse_and_validate(&manifest_bytes)?;
    let manifest_sha256 = hex(&Sha256::digest(&manifest_bytes));
    let signature = read_signature(&canonical_root)?;
    let resource_count = manifest
        .modules
        .iter()
        .flat_map(|module| &module.lessons)
        .map(|lesson| lesson.resources.len())
        .sum::<usize>();
    if resource_count > limits.max_resource_count {
        return Err(ImportError::LimitExceeded(format!(
            "declares {resource_count} resources; limit is {}",
            limits.max_resource_count
        )));
    }

    let total_declared_bytes = manifest
        .modules
        .iter()
        .flat_map(|module| &module.lessons)
        .flat_map(|lesson| &lesson.resources)
        .try_fold(0_u64, |total, resource| {
            total.checked_add(resource.byte_length)
        })
        .ok_or_else(|| {
            ImportError::LimitExceeded("declared resource bytes overflow u64".to_owned())
        })?;
    if total_declared_bytes > limits.max_total_resource_bytes {
        return Err(ImportError::LimitExceeded(format!(
            "declares {total_declared_bytes} resource bytes; limit is {}",
            limits.max_total_resource_bytes
        )));
    }

    let mut resources = Vec::with_capacity(resource_count);
    for resource in manifest
        .modules
        .iter()
        .flat_map(|module| &module.lessons)
        .flat_map(|lesson| &lesson.resources)
    {
        cancellation.check()?;
        resources.push(verify_resource(
            &canonical_root,
            &resource.id,
            &resource.path,
            resource.byte_length,
            &resource.sha256,
            cancellation,
        )?);
    }

    Ok(ImportedCourse {
        manifest,
        resources,
        total_resource_bytes: total_declared_bytes,
        manifest_sha256,
        signature,
    })
}

/// The package's `zeron-signature.json`, if it has one: a regular file, not
/// a symbolic link, within the signature size bound.
fn read_signature(root: &Path) -> Result<Option<Vec<u8>>, ImportError> {
    let path = root.join(course_signing::RELEASE_SIGNATURE_FILE);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ImportError::Io {
                operation: "reading the signature file metadata",
                source,
            });
        }
    };
    let limit = course_signing::MAX_RELEASE_SIGNATURE_BYTES as u64;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > limit {
        return Err(ImportError::InvalidManifestFile(
            "zeron-signature.json must be a small regular file, not a symbolic link".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    File::open(&path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|source| ImportError::Io {
            operation: "reading the signature file",
            source,
        })?;
    if bytes.len() as u64 > limit {
        return Err(ImportError::InvalidManifestFile(
            "zeron-signature.json is too large".to_owned(),
        ));
    }
    Ok(Some(bytes))
}

fn read_manifest(
    root: &Path,
    limits: ImportLimits,
    cancellation: &CancellationFlag,
) -> Result<Vec<u8>, ImportError> {
    cancellation.check()?;
    let path = root.join(MANIFEST_FILE_NAME);
    let metadata = fs::symlink_metadata(&path).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            ImportError::InvalidManifestFile("course.json is missing".to_owned())
        } else {
            ImportError::Io {
                operation: "reading course.json metadata",
                source,
            }
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ImportError::InvalidManifestFile(
            "course.json must be a regular file, not a symbolic link".to_owned(),
        ));
    }
    if metadata.len() > limits.max_manifest_bytes {
        return Err(ImportError::ManifestTooLarge {
            actual: metadata.len(),
            limit: limits.max_manifest_bytes,
        });
    }
    let capacity = usize::try_from(metadata.len()).map_err(|_| ImportError::ManifestTooLarge {
        actual: metadata.len(),
        limit: limits.max_manifest_bytes,
    })?;
    let file = File::open(path).map_err(|source| ImportError::Io {
        operation: "opening course.json",
        source,
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(limits.max_manifest_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| ImportError::Io {
            operation: "reading course.json",
            source,
        })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limits.max_manifest_bytes {
        return Err(ImportError::ManifestTooLarge {
            actual: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            limit: limits.max_manifest_bytes,
        });
    }
    cancellation.check()?;
    Ok(bytes)
}

fn verify_resource(
    root: &Path,
    resource_id: &str,
    package_path: &str,
    expected_length: u64,
    expected_sha256: &str,
    cancellation: &CancellationFlag,
) -> Result<VerifiedResource, ImportError> {
    let candidate = checked_join(root, resource_id, package_path)?;
    reject_symlink_components(root, &candidate, resource_id, package_path)?;
    let canonical = fs::canonicalize(&candidate).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            ImportError::MissingResource {
                resource_id: resource_id.to_owned(),
                package_path: package_path.to_owned(),
            }
        } else {
            ImportError::Io {
                operation: "resolving a course resource",
                source,
            }
        }
    })?;
    if !canonical.starts_with(root) {
        return Err(unsafe_resource(
            resource_id,
            package_path,
            "resolved outside the course root",
        ));
    }

    let file = File::open(&canonical).map_err(|source| ImportError::Io {
        operation: "opening a course resource",
        source,
    })?;
    let before = file.metadata().map_err(|source| ImportError::Io {
        operation: "reading resource metadata",
        source,
    })?;
    if !before.is_file() {
        return Err(ImportError::MissingResource {
            resource_id: resource_id.to_owned(),
            package_path: package_path.to_owned(),
        });
    }
    if before.len() != expected_length {
        return Err(ImportError::ResourceMismatch {
            resource_id: resource_id.to_owned(),
            field: "byte length",
            expected: expected_length.to_string(),
            actual: before.len().to_string(),
        });
    }

    let digest = hash_file(file, cancellation)?;
    let after = fs::metadata(&canonical).map_err(|source| ImportError::Io {
        operation: "rechecking resource metadata",
        source,
    })?;
    if metadata_changed(&before, &after) {
        return Err(ImportError::SourceChanged {
            resource_id: resource_id.to_owned(),
        });
    }
    if digest != expected_sha256 {
        return Err(ImportError::ResourceMismatch {
            resource_id: resource_id.to_owned(),
            field: "SHA-256",
            expected: expected_sha256.to_owned(),
            actual: digest,
        });
    }

    Ok(VerifiedResource {
        resource_id: resource_id.to_owned(),
        package_path: package_path.to_owned(),
        byte_length: before.len(),
        sha256: expected_sha256.to_owned(),
    })
}

fn checked_join(
    root: &Path,
    resource_id: &str,
    package_path: &str,
) -> Result<PathBuf, ImportError> {
    let relative = Path::new(package_path);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(unsafe_resource(
            resource_id,
            package_path,
            "contains a non-normal path component",
        ));
    }
    Ok(root.join(relative))
}

fn reject_symlink_components(
    root: &Path,
    candidate: &Path,
    resource_id: &str,
    package_path: &str,
) -> Result<(), ImportError> {
    let relative = candidate.strip_prefix(root).map_err(|_| {
        unsafe_resource(
            resource_id,
            package_path,
            "does not begin at the course root",
        )
    })?;
    let mut current = root.to_owned();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(unsafe_resource(
                resource_id,
                package_path,
                "contains a non-normal path component",
            ));
        };
        current.push(segment);
        let metadata = fs::symlink_metadata(&current).map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                ImportError::MissingResource {
                    resource_id: resource_id.to_owned(),
                    package_path: package_path.to_owned(),
                }
            } else {
                ImportError::Io {
                    operation: "checking resource path components",
                    source,
                }
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(unsafe_resource(
                resource_id,
                package_path,
                "contains a symbolic link",
            ));
        }
    }
    Ok(())
}

fn hash_file(file: File, cancellation: &CancellationFlag) -> Result<String, ImportError> {
    let mut reader = BufReader::with_capacity(HASH_BUFFER_BYTES, file);
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES];
    let mut hasher = Sha256::new();
    loop {
        cancellation.check()?;
        let read = reader.read(&mut buffer).map_err(|source| ImportError::Io {
            operation: "hashing a course resource",
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn metadata_changed(before: &Metadata, after: &Metadata) -> bool {
    before.len() != after.len() || modified(before) != modified(after)
}

fn modified(metadata: &Metadata) -> Option<SystemTime> {
    metadata.modified().ok()
}

fn unsafe_resource(resource_id: &str, package_path: &str, reason: &str) -> ImportError {
    ImportError::UnsafeResource {
        resource_id: resource_id.to_owned(),
        package_path: package_path.to_owned(),
        reason: reason.to_owned(),
    }
}
