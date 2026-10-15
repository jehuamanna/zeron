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

use course_format::{
    COURSE_SCHEMA_VERSION, CourseManifest, Lesson, License, ManifestError, Module, Publisher,
    Resource, ValidationIssue,
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
    /// Copied byte length.
    pub byte_length: u64,
    /// Lowercase SHA-256 of the copied bytes.
    pub sha256: String,
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
    /// Sum of resource sizes.
    pub total_resource_bytes: u64,
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
/// Ordering follows the draft exactly; positions are derived from that order.
#[must_use]
pub fn manifest_for(
    draft: &DraftCourse,
    mut digest: impl FnMut(&DraftResource) -> (u64, String),
) -> CourseManifest {
    let metadata = &draft.metadata;
    CourseManifest {
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
                                let (byte_length, sha256) = digest(resource);
                                Resource {
                                    id: resource.id.clone(),
                                    kind: resource.kind,
                                    path: resource.package_path.clone(),
                                    media_type: resource.media_type.clone(),
                                    byte_length,
                                    sha256,
                                    duration_seconds: resource.duration_seconds.map(seconds),
                                    language: resource.language.clone(),
                                    relations: Vec::new(),
                                }
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    }
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
    cancellation.check()?;
    let issues = draft.validate();
    if !issues.is_empty() {
        return Err(PackagingError::DraftInvalid(issues));
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
    let result = write_package(draft, destination, &plan, cancellation, progress);
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

struct PlannedSource<'a> {
    resource: &'a DraftResource,
    byte_length: u64,
    modified: Option<SystemTime>,
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
    let resources: Vec<&DraftResource> = draft
        .modules
        .iter()
        .flat_map(|module| &module.lessons)
        .flat_map(|lesson| &lesson.resources)
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
    for resource in resources {
        let metadata = fs::metadata(&resource.source_path)
            .ok()
            .filter(fs::Metadata::is_file)
            .ok_or_else(|| PackagingError::SourceUnavailable {
                resource_id: resource.id.clone(),
            })?;
        total_bytes = total_bytes
            .checked_add(metadata.len())
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
) -> Result<StagedPackage, PackagingError> {
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
        let (byte_length, sha256) =
            copy_and_hash(source, &target, cancellation, &mut state, progress)?;
        let after = fs::metadata(&resource.source_path).ok();
        let unchanged = byte_length == source.byte_length
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
        });
    }

    let manifest = manifest_for(draft, |resource| {
        files
            .iter()
            .find(|file| file.resource_id == resource.id)
            .map(|file| (file.byte_length, file.sha256.clone()))
            .unwrap_or_default()
    });
    validate_manifest(&manifest)?;
    let bytes = canonical_manifest_bytes(&manifest).map_err(|error| PackagingError::Io {
        operation: "serializing course.json",
        source: io::Error::other(error),
    })?;
    write_new_file(&destination.join(MANIFEST_FILE_NAME), &bytes)?;
    cancellation.check()?;
    Ok(StagedPackage {
        manifest,
        manifest_sha256: hex(&Sha256::digest(&bytes)),
        files,
        total_resource_bytes: plan.total_bytes,
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
