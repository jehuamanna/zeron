use std::{
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

use course_access::{AccessError, ContentKey, ResourceCipher};
use course_format::{
    AccessModel, COURSE_SCHEMA_VERSION, CourseAccess, CourseManifest, Lesson, License,
    ManifestError, Module, Publisher, Resource, ResourceEncryption, ValidationIssue,
    sealed_byte_length,
};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{DraftCourse, DraftResource, DraftValidationIssue};

/// Manifest file name at the package root.
pub const MANIFEST_FILE_NAME: &str = "course.json";
const COPY_BUFFER_BYTES: usize = 1024 * 1024;
const PLACEHOLDER_SHA256: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Upper bounds applied before any bytes are copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackagingLimits {
    /// Maximum number of resources in one package.
    pub max_resource_count: usize,
    /// Maximum sum of resource sizes.
    pub max_total_bytes: u64,
}

impl Default for PackagingLimits {
    fn default() -> Self {
        Self {
            max_resource_count: 10_000,
            max_total_bytes: 4 * 1024 * 1024 * 1024 * 1024,
        }
    }
}

/// Cooperative cancellation checked between files and copy chunks.
#[derive(Debug, Clone, Default)]
pub struct PackagingCancellation(Arc<AtomicBool>);

impl PackagingCancellation {
    /// Request cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn check(&self) -> Result<(), PackagingError> {
        if self.is_cancelled() {
            Err(PackagingError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Copy progress reported after every chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StagingProgress {
    /// Resources fully copied and hashed.
    pub completed_files: usize,
    /// Resources in the package.
    pub total_files: usize,
    /// Bytes copied so far.
    pub completed_bytes: u64,
    /// Bytes in all resources.
    pub total_bytes: u64,
}

/// Verified facts about one staged resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedFile {
    /// Stable resource ID.
    pub resource_id: String,
    /// Package-relative path.
    pub package_path: String,
    /// Byte length in the package (sealed, if encrypted).
    pub byte_length: u64,
    /// Lowercase SHA-256 of the package bytes (sealed, if encrypted).
    pub sha256: String,
    /// Source length, when the resource was sealed.
    pub plaintext_length: Option<u64>,
}

/// A complete staged package and its canonical manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedPackage {
    /// Validated manifest written as `course.json`.
    pub manifest: CourseManifest,
    /// Lowercase SHA-256 of the exact `course.json` bytes.
    pub manifest_sha256: String,
    /// Every staged resource in manifest order.
    pub files: Vec<StagedFile>,
    /// Sum of resource sizes in the package.
    pub total_resource_bytes: u64,
    /// The signature file written beside `course.json`, if the package is signed.
    pub signature_file: Option<String>,
}

/// Classified staging failure. No variant carries an absolute path.
#[derive(Debug, Error)]
pub enum PackagingError {
    /// The caller cancelled staging.
    #[error("packaging was cancelled")]
    Cancelled,
    /// The draft has blocking validation issues.
    #[error("the draft has {} blocking issue(s)", .0.len())]
    DraftInvalid(Vec<DraftValidationIssue>),
    /// The generated manifest violates the course format.
    #[error("the generated course.json is invalid with {} issue(s)", .0.len())]
    Manifest(Vec<ValidationIssue>),
    /// A configured bound was exceeded.
    #[error("package exceeds limit: {0}")]
    LimitExceeded(String),
    /// A requested package path could escape the staging directory.
    #[error("resource `{resource_id}` has an unsafe package path")]
    UnsafePackagePath {
        /// Stable resource ID.
        resource_id: String,
    },
    /// A source is missing or not a regular file.
    #[error("the source for resource `{resource_id}` is missing or not a regular file")]
    SourceUnavailable {
        /// Stable resource ID.
        resource_id: String,
    },
    /// A source changed while it was being copied.
    #[error("the source for resource `{resource_id}` changed while it was copied")]
    SourceChanged {
        /// Stable resource ID.
        resource_id: String,
    },
    /// The draft has paid lessons but no content key was supplied.
    #[error("a content key is required to package paid lessons")]
    ContentKeyRequired,
    /// The staging destination already exists.
    #[error("the staging destination already exists")]
    DestinationExists,
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

/// Build the canonical manifest for a draft and per-resource digests.
///
/// `digest` returns a resource's source length and the SHA-256 of its package
/// bytes. Resources of paid lessons get the sealed `byteLength` and an
/// `encryption` record, and the course gets `access` and schema version 2.
/// Ordering follows the draft exactly; positions are derived from that order.
#[must_use]
pub fn manifest_for(
    draft: &DraftCourse,
    mut digest: impl FnMut(&DraftResource) -> (u64, String),
) -> CourseManifest {
    let metadata = &draft.metadata;
    let mut manifest = CourseManifest {
        schema_version: COURSE_SCHEMA_VERSION,
        course_id: metadata.course_id.clone(),
        version: metadata.version.clone(),
        title: metadata.title.clone(),
        description: metadata.description.clone(),
        publisher: Publisher {
            name: metadata.publisher_name.clone(),
            publisher_id: None,
        },
        language: metadata.language.clone(),
        category: metadata.category.clone(),
        tags: metadata.tags.clone(),
        license: metadata
            .license_name
            .clone()
            .map(|name| License { name, url: None }),
        prerequisites: metadata.prerequisites.clone(),
        references: Vec::new(),
        thumbnail_resource_id: None,
        estimated_duration_seconds: None,
        modules: draft
            .modules
            .iter()
            .zip(0_u32..)
            .map(|(module, position)| Module {
                id: module.id.clone(),
                title: module.title.clone(),
                position,
                lessons: module
                    .lessons
                    .iter()
                    .zip(0_u32..)
                    .map(|(lesson, position)| Lesson {
                        id: lesson.id.clone(),
                        title: lesson.title.clone(),
                        position,
                        estimated_duration_seconds: lesson.estimated_duration_seconds.map(seconds),
                        resources: lesson
                            .resources
                            .iter()
                            .map(|resource| {
                                let (source_length, sha256) = digest(resource);
                                let encryption = lesson
                                    .paid
                                    .then(|| ResourceEncryption::chunked(source_length));
                                Resource {
                                    id: resource.id.clone(),
                                    kind: resource.kind,
                                    path: resource.package_path.clone(),
                                    media_type: resource.media_type.clone(),
                                    byte_length: if lesson.paid {
                                        sealed_byte_length(source_length)
                                    } else {
                                        source_length
                                    },
                                    sha256,
                                    duration_seconds: resource.duration_seconds.map(seconds),
                                    language: resource.language.clone(),
                                    relations: Vec::new(),
                                    encryption,
                                }
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
        access: None,
    };
    if manifest
        .resources()
        .any(|resource| resource.encryption.is_some())
    {
        manifest.access = Some(CourseAccess {
            model: AccessModel::Paid,
        });
    }
    manifest.schema_version = manifest.required_schema_version();
    manifest
}

/// Serialize a manifest to its canonical bytes: pretty JSON in struct field
/// order with a trailing newline.
///
/// # Errors
/// Returns an error only if serialization itself fails.
pub fn canonical_manifest_bytes(manifest: &CourseManifest) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec_pretty(manifest)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// How to sign a staged package. Crypto stays with the caller: `sign`
/// receives the final manifest and its SHA-256 and returns the bytes of the
/// signature file.
pub struct PackageSigning<'a> {
    /// Written into `course.json` as `publisher.publisherId`.
    pub publisher_id: String,
    /// The signature file's name at the package root.
    pub file_name: &'a str,
    /// Produce the signature file for a manifest and its digest.
    pub sign: &'a dyn Fn(&CourseManifest, &str) -> Vec<u8>,
}

/// Copy every draft source into a new `destination` directory, hash it, and
/// write the canonical `course.json`.
///
/// Sources are only read. `destination` must not exist; on any failure or
/// cancellation the partially written destination is removed.
///
/// # Errors
/// Returns a classified [`PackagingError`].
pub fn stage_package(
    draft: &DraftCourse,
    destination: &Path,
    limits: PackagingLimits,
    cancellation: &PackagingCancellation,
    progress: &mut dyn FnMut(StagingProgress),
) -> Result<StagedPackage, PackagingError> {
    stage_signed_package(draft, destination, limits, cancellation, progress, None)
}

/// [`stage_package`], then, with `signing`, name the publisher in
/// `course.json` and write the signature file beside it.
///
/// # Errors
/// Returns a classified [`PackagingError`].
pub fn stage_signed_package(
    draft: &DraftCourse,
    destination: &Path,
    limits: PackagingLimits,
    cancellation: &PackagingCancellation,
    progress: &mut dyn FnMut(StagingProgress),
    signing: Option<&PackageSigning<'_>>,
) -> Result<StagedPackage, PackagingError> {
    stage_package_with(
        draft,
        destination,
        limits,
        cancellation,
        progress,
        PackageOptions {
            signing,
            content_key: None,
        },
    )
}

/// Optional signing and encryption for [`stage_package_with`].
#[derive(Default, Clone, Copy)]
pub struct PackageOptions<'a> {
    /// Name the publisher and write the signature file.
    pub signing: Option<&'a PackageSigning<'a>>,
    /// The release's content key. Required when the draft has paid lessons;
    /// their resources are sealed under it (ADR 0017).
    pub content_key: Option<&'a ContentKey>,
}

/// [`stage_signed_package`], sealing every resource of a paid lesson under
/// `options.content_key`.
///
/// # Errors
/// Returns a classified [`PackagingError`], including
/// [`PackagingError::ContentKeyRequired`] for paid lessons without a key.
pub fn stage_package_with(
    draft: &DraftCourse,
    destination: &Path,
    limits: PackagingLimits,
    cancellation: &PackagingCancellation,
    progress: &mut dyn FnMut(StagingProgress),
    options: PackageOptions<'_>,
) -> Result<StagedPackage, PackagingError> {
    cancellation.check()?;
    let issues = draft.validate();
    if !issues.is_empty() {
        return Err(PackagingError::DraftInvalid(issues));
    }
    if draft.has_paid_content() && options.content_key.is_none() {
        return Err(PackagingError::ContentKeyRequired);
    }
    let plan = plan(draft, limits)?;
    validate_manifest(&manifest_for(draft, |resource| {
        (plan.length_of(&resource.id), PLACEHOLDER_SHA256.to_owned())
    }))?;

    fs::create_dir(destination).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            PackagingError::DestinationExists
        } else {
            PackagingError::Io {
                operation: "creating the staging directory",
                source,
            }
        }
    })?;
    let result = write_package(draft, destination, &plan, cancellation, progress, options);
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

struct PlannedSource<'a> {
    resource: &'a DraftResource,
    byte_length: u64,
    modified: Option<SystemTime>,
    paid: bool,
}

struct Plan<'a> {
    sources: Vec<PlannedSource<'a>>,
    total_bytes: u64,
}

impl Plan<'_> {
    fn length_of(&self, resource_id: &str) -> u64 {
        self.sources
            .iter()
            .find(|source| source.resource.id == resource_id)
            .map_or(0, |source| source.byte_length)
    }
}

fn plan(draft: &DraftCourse, limits: PackagingLimits) -> Result<Plan<'_>, PackagingError> {
    let resources: Vec<(&DraftResource, bool)> = draft
        .modules
        .iter()
        .flat_map(|module| &module.lessons)
        .flat_map(|lesson| {
            lesson
                .resources
                .iter()
                .map(|resource| (resource, lesson.paid))
        })
        .collect();
    if resources.len() > limits.max_resource_count {
        return Err(PackagingError::LimitExceeded(format!(
            "{} resources; limit is {}",
            resources.len(),
            limits.max_resource_count
        )));
    }
    let mut total_bytes = 0_u64;
    let mut sources = Vec::with_capacity(resources.len());
    for (resource, paid) in resources {
        let metadata = fs::metadata(&resource.source_path)
            .ok()
            .filter(fs::Metadata::is_file)
            .ok_or_else(|| PackagingError::SourceUnavailable {
                resource_id: resource.id.clone(),
            })?;
        let package_length = if paid {
            sealed_byte_length(metadata.len())
        } else {
            metadata.len()
        };
        total_bytes = total_bytes
            .checked_add(package_length)
            .filter(|total| *total <= limits.max_total_bytes)
            .ok_or_else(|| {
                PackagingError::LimitExceeded(format!(
                    "resources exceed {} bytes",
                    limits.max_total_bytes
                ))
            })?;
        sources.push(PlannedSource {
            resource,
            byte_length: metadata.len(),
            modified: metadata.modified().ok(),
            paid,
        });
    }
    Ok(Plan {
        sources,
        total_bytes,
    })
}

fn write_package(
    draft: &DraftCourse,
    destination: &Path,
    plan: &Plan<'_>,
    cancellation: &PackagingCancellation,
    progress: &mut dyn FnMut(StagingProgress),
    options: PackageOptions<'_>,
) -> Result<StagedPackage, PackagingError> {
    let signing = options.signing;
    let mut state = StagingProgress {
        completed_files: 0,
        total_files: plan.sources.len(),
        completed_bytes: 0,
        total_bytes: plan.total_bytes,
    };
    progress(state);
    let mut files = Vec::with_capacity(plan.sources.len());
    for source in &plan.sources {
        cancellation.check()?;
        let resource = source.resource;
        let target = package_target(destination, resource)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|source| PackagingError::Io {
                operation: "creating a package directory",
                source,
            })?;
        }
        let (byte_length, sha256) = match options.content_key.filter(|_| source.paid) {
            Some(key) => seal_and_hash(source, key, &target, cancellation, &mut state, progress)?,
            None => copy_and_hash(source, &target, cancellation, &mut state, progress)?,
        };
        let expected_length = if source.paid {
            sealed_byte_length(source.byte_length)
        } else {
            source.byte_length
        };
        let after = fs::metadata(&resource.source_path).ok();
        let unchanged = byte_length == expected_length
            && after.is_some_and(|metadata| {
                metadata.len() == source.byte_length && metadata.modified().ok() == source.modified
            });
        if !unchanged {
            return Err(PackagingError::SourceChanged {
                resource_id: resource.id.clone(),
            });
        }
        state.completed_files += 1;
        progress(state);
        files.push(StagedFile {
            resource_id: resource.id.clone(),
            package_path: resource.package_path.clone(),
            byte_length,
            sha256,
            plaintext_length: source.paid.then_some(source.byte_length),
        });
    }

    let mut manifest = manifest_for(draft, |resource| {
        files
            .iter()
            .find(|file| file.resource_id == resource.id)
            .map(|file| {
                (
                    file.plaintext_length.unwrap_or(file.byte_length),
                    file.sha256.clone(),
                )
            })
            .unwrap_or_default()
    });
    if let Some(signing) = signing {
        manifest.publisher.publisher_id = Some(signing.publisher_id.clone());
    }
    validate_manifest(&manifest)?;
    let bytes = canonical_manifest_bytes(&manifest).map_err(|error| PackagingError::Io {
        operation: "serializing course.json",
        source: io::Error::other(error),
    })?;
    write_new_file(&destination.join(MANIFEST_FILE_NAME), &bytes)?;
    let manifest_sha256 = hex(&Sha256::digest(&bytes));
    let signature_file = match signing {
        Some(signing) => {
            let reserved = files
                .iter()
                .any(|file| file.package_path == signing.file_name);
            if reserved {
                return Err(PackagingError::LimitExceeded(format!(
                    "a resource uses the reserved name {}",
                    signing.file_name
                )));
            }
            let signature = (signing.sign)(&manifest, &manifest_sha256);
            write_new_file(&destination.join(signing.file_name), &signature)?;
            Some(signing.file_name.to_owned())
        }
        None => None,
    };
    cancellation.check()?;
    Ok(StagedPackage {
        manifest,
        manifest_sha256,
        files,
        total_resource_bytes: plan.total_bytes,
        signature_file,
    })
}

fn package_target(root: &Path, resource: &DraftResource) -> Result<PathBuf, PackagingError> {
    let unsafe_path = || PackagingError::UnsafePackagePath {
        resource_id: resource.id.clone(),
    };
    let mut target = root.to_path_buf();
    for segment in resource.package_path.split('/') {
        let mut components = Path::new(segment).components();
        let single_normal =
            matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
        if !single_normal || segment.contains(['\\', ':', '\0']) {
            return Err(unsafe_path());
        }
        target.push(segment);
    }
    if resource
        .package_path
        .eq_ignore_ascii_case(MANIFEST_FILE_NAME)
    {
        return Err(unsafe_path());
    }
    Ok(target)
}

fn copy_and_hash(
    source: &PlannedSource<'_>,
    target: &Path,
    cancellation: &PackagingCancellation,
    state: &mut StagingProgress,
    progress: &mut dyn FnMut(StagingProgress),
) -> Result<(u64, String), PackagingError> {
    let mut input = File::open(&source.resource.source_path).map_err(|_| {
        PackagingError::SourceUnavailable {
            resource_id: source.resource.id.clone(),
        }
    })?;
    let mut output = create_new(target)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut copied = 0_u64;
    loop {
        cancellation.check()?;
        let read = input
            .read(&mut buffer)
            .map_err(|source| PackagingError::Io {
                operation: "reading a source file",
                source,
            })?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        copied = copied.saturating_add(read as u64);
        if copied > source.byte_length {
            return Err(PackagingError::SourceChanged {
                resource_id: source.resource.id.clone(),
            });
        }
        hasher.update(chunk);
        output
            .write_all(chunk)
            .map_err(|source| PackagingError::Io {
                operation: "writing a staged file",
                source,
            })?;
        state.completed_bytes = state.completed_bytes.saturating_add(read as u64);
        progress(*state);
    }
    output.sync_all().map_err(|source| PackagingError::Io {
        operation: "flushing a staged file",
        source,
    })?;
    Ok((copied, hex(&hasher.finalize())))
}

/// Seal a paid resource into the package, hashing the sealed bytes. Reads
/// exactly the planned length; a source that shrank or grew is reported as
/// changed.
fn seal_and_hash(
    source: &PlannedSource<'_>,
    key: &ContentKey,
    target: &Path,
    cancellation: &PackagingCancellation,
    state: &mut StagingProgress,
    progress: &mut dyn FnMut(StagingProgress),
) -> Result<(u64, String), PackagingError> {
    let resource_id = &source.resource.id;
    let input = File::open(&source.resource.source_path).map_err(|_| {
        PackagingError::SourceUnavailable {
            resource_id: resource_id.clone(),
        }
    })?;
    let mut output = HashingWriter {
        file: create_new(target)?,
        hasher: Sha256::new(),
        state,
        progress,
    };
    let mut reader = CancellableReader {
        inner: io::BufReader::with_capacity(COPY_BUFFER_BYTES, input),
        cancellation,
    };
    let cipher = ResourceCipher::new(key, resource_id, source.byte_length);
    let sealed = cipher.seal(&mut reader, &mut output).map_err(|error| {
        if cancellation.is_cancelled() {
            return PackagingError::Cancelled;
        }
        match error {
            AccessError::Io(source) => PackagingError::Io {
                operation: "sealing a staged file",
                source,
            },
            AccessError::Length | AccessError::Authentication => PackagingError::SourceChanged {
                resource_id: resource_id.clone(),
            },
        }
    })?;
    output
        .file
        .sync_all()
        .map_err(|source| PackagingError::Io {
            operation: "flushing a staged file",
            source,
        })?;
    Ok((sealed, hex(&output.hasher.finalize())))
}

struct CancellableReader<'a, R> {
    inner: R,
    cancellation: &'a PackagingCancellation,
}

impl<R: Read> Read for CancellableReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancellation.is_cancelled() {
            return Err(io::Error::other("packaging was cancelled"));
        }
        self.inner.read(buffer)
    }
}

struct HashingWriter<'a, 'p> {
    file: File,
    hasher: Sha256,
    state: &'a mut StagingProgress,
    progress: &'a mut (dyn FnMut(StagingProgress) + 'p),
}

impl Write for HashingWriter<'_, '_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.file.write(bytes)?;
        self.hasher.update(&bytes[..written]);
        self.state.completed_bytes = self.state.completed_bytes.saturating_add(written as u64);
        (self.progress)(*self.state);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), PackagingError> {
    let mut file = create_new(path)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| PackagingError::Io {
            operation: "writing course.json",
            source,
        })
}

fn create_new(path: &Path) -> Result<File, PackagingError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| PackagingError::Io {
            operation: "creating a staged file",
            source,
        })
}

fn validate_manifest(manifest: &CourseManifest) -> Result<(), PackagingError> {
    match manifest.validate() {
        Ok(()) => Ok(()),
        Err(ManifestError::Validation(issues)) => Err(PackagingError::Manifest(issues)),
        Err(ManifestError::Json(error)) => Err(PackagingError::Io {
            operation: "validating course.json",
            source: io::Error::other(error),
        }),
    }
}

#[allow(clippy::cast_precision_loss)]
fn seconds(value: u64) -> f64 {
    value as f64
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}
