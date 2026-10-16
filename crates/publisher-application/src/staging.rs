use std::{fs, io, path::Path};

use course_access::ContentKey;
use course_publisher::{
    DraftCourse, PackageOptions, PackageSigning, PackagingCancellation, PackagingError,
    PackagingLimits, StagingProgress, stage_package_with,
};

use crate::{DraftServiceError, ReleaseSigner, opaque_id, signing::SIGNATURE_FILE};

const ATTEMPT_PREFIX: &str = "stage_";
const PARTIAL_SUFFIX: &str = ".partial";

/// Path-free summary of a completed staging attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagingReceipt {
    /// Opaque attempt ID; also the completed directory name under the staging root.
    pub attempt_id: String,
    /// Draft revision frozen into this package.
    pub draft_revision: u64,
    /// Logical course ID.
    pub course_id: String,
    /// Course version written to `course.json`.
    pub course_version: String,
    /// Number of staged resources.
    pub resource_count: usize,
    /// Sum of staged resource sizes.
    pub total_resource_bytes: u64,
    /// SHA-256 of the exact `course.json` bytes.
    pub manifest_sha256: String,
    /// Whether the package carries the publisher's signature.
    pub signed: bool,
    /// Whether paid lessons were sealed under a content key.
    pub paid: bool,
}

/// Secrets a staging attempt may use. Both stay behind the Rust boundary.
#[derive(Default, Clone, Copy)]
pub struct ReleaseSecrets<'a> {
    /// Signs `course.json` with the publisher's key.
    pub signer: Option<&'a ReleaseSigner>,
    /// Seals paid lessons (ADR 0017); required when the draft has any.
    pub content_key: Option<&'a ContentKey>,
}

/// Stage a frozen draft snapshot into a new attempt under `staging_root`.
///
/// Work happens in `<attempt>.partial`, renamed to `<attempt>` only after the
/// package and manifest are complete, so an interrupted run is recognizable by
/// [`discard_abandoned_staging`]. Callers should not hold draft storage locks
/// while this runs.
///
/// # Errors
/// Returns [`DraftServiceError::Packaging`] for staging failures, including cancellation.
pub fn stage_draft(
    draft: &DraftCourse,
    staging_root: &Path,
    limits: PackagingLimits,
    cancellation: &PackagingCancellation,
    progress: &mut dyn FnMut(StagingProgress),
) -> Result<StagingReceipt, DraftServiceError> {
    stage_signed_draft(draft, staging_root, limits, cancellation, progress, None)
}

/// [`stage_draft`], signing the package with `signer` when one is given:
/// `course.json` names the publisher and `zeron-signature.json` signs it.
///
/// # Errors
/// As [`stage_draft`].
pub fn stage_signed_draft(
    draft: &DraftCourse,
    staging_root: &Path,
    limits: PackagingLimits,
    cancellation: &PackagingCancellation,
    progress: &mut dyn FnMut(StagingProgress),
    signer: Option<&ReleaseSigner>,
) -> Result<StagingReceipt, DraftServiceError> {
    stage_draft_with(
        draft,
        staging_root,
        limits,
        cancellation,
        progress,
        ReleaseSecrets {
            signer,
            content_key: None,
        },
    )
}

/// [`stage_signed_draft`], sealing paid lessons under `secrets.content_key`.
///
/// # Errors
/// As [`stage_draft`].
pub fn stage_draft_with(
    draft: &DraftCourse,
    staging_root: &Path,
    limits: PackagingLimits,
    cancellation: &PackagingCancellation,
    progress: &mut dyn FnMut(StagingProgress),
    secrets: ReleaseSecrets<'_>,
) -> Result<StagingReceipt, DraftServiceError> {
    let signer = secrets.signer;
    fs::create_dir_all(staging_root)
        .map_err(|source| io_error("creating the staging root", source))?;
    let attempt_id = opaque_id("stage")?;
    let partial = staging_root.join(format!("{attempt_id}{PARTIAL_SUFFIX}"));
    let sign = |manifest: &course_format::CourseManifest, manifest_sha256: &str| {
        signer.map_or_else(Vec::new, |signer| {
            signer.sign(&manifest.course_id, &manifest.version, manifest_sha256)
        })
    };
    let signing = signer.map(|signer| PackageSigning {
        publisher_id: signer.publisher_id().to_owned(),
        file_name: SIGNATURE_FILE,
        sign: &sign,
    });
    let staged = stage_package_with(
        draft,
        &partial,
        limits,
        cancellation,
        progress,
        PackageOptions {
            signing: signing.as_ref(),
            content_key: secrets.content_key,
        },
    )
    .map_err(DraftServiceError::Packaging)?;
    if let Err(source) = fs::rename(&partial, staging_root.join(&attempt_id)) {
        let _ = fs::remove_dir_all(&partial);
        return Err(io_error("completing the staging attempt", source));
    }
    Ok(StagingReceipt {
        signed: staged.signature_file.is_some(),
        paid: staged.manifest.access.is_some(),
        attempt_id,
        draft_revision: draft.revision,
        course_id: staged.manifest.course_id,
        course_version: staged.manifest.version,
        resource_count: staged.files.len(),
        total_resource_bytes: staged.total_resource_bytes,
        manifest_sha256: staged.manifest_sha256,
    })
}

/// Remove interrupted staging attempts left by a crash or forced exit.
///
/// Only real directories named exactly `stage_<32 lowercase hex>.partial` and
/// regular files named `stage_<32 lowercase hex>.torrent.partial` directly
/// under `staging_root` are removed. Completed attempts, torrents, symbolic
/// links, and anything else are left untouched.
///
/// # Errors
/// Returns an I/O failure when the root cannot be listed or an attempt cannot be removed.
pub fn discard_abandoned_staging(staging_root: &Path) -> Result<usize, DraftServiceError> {
    let entries = match fs::read_dir(staging_root) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(source) => return Err(io_error("listing the staging root", source)),
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|source| io_error("listing the staging root", source))?;
        let file_type = entry
            .file_type()
            .map_err(|source| io_error("inspecting a staging attempt", source))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if file_type.is_dir() && is_partial_attempt(&name) {
            fs::remove_dir_all(entry.path())
                .map_err(|source| io_error("removing an abandoned staging attempt", source))?;
            removed += 1;
        } else if file_type.is_file() && is_partial_torrent(&name) {
            fs::remove_file(entry.path())
                .map_err(|source| io_error("removing an abandoned torrent file", source))?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Remove every staging attempt and torrent, complete or not.
///
/// Preparations are session-scoped: once the process that prepared them has
/// exited, nothing can activate them, so they are derived data safe to drop.
/// Only names matching the attempt patterns directly under `staging_root`
/// are touched; symbolic links and other entries are left alone.
///
/// # Errors
/// Returns an I/O failure when the root cannot be listed or an entry cannot be removed.
pub fn discard_all_attempts(staging_root: &Path) -> Result<usize, DraftServiceError> {
    let entries = match fs::read_dir(staging_root) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(source) => return Err(io_error("listing the staging root", source)),
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|source| io_error("listing the staging root", source))?;
        let file_type = entry
            .file_type()
            .map_err(|source| io_error("inspecting a staging attempt", source))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let attempt_directory =
            file_type.is_dir() && (is_completed_attempt(&name) || is_partial_attempt(&name));
        let attempt_torrent = file_type.is_file()
            && (is_partial_torrent(&name)
                || name
                    .strip_suffix(".torrent")
                    .is_some_and(is_completed_attempt));
        if attempt_directory {
            fs::remove_dir_all(entry.path())
                .map_err(|source| io_error("removing a staging attempt", source))?;
            removed += 1;
        } else if attempt_torrent {
            fs::remove_file(entry.path())
                .map_err(|source| io_error("removing a staging torrent", source))?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn is_partial_attempt(name: &str) -> bool {
    name.strip_suffix(PARTIAL_SUFFIX)
        .is_some_and(is_completed_attempt)
}

fn is_partial_torrent(name: &str) -> bool {
    name.strip_suffix(".torrent.partial")
        .is_some_and(is_completed_attempt)
}

/// Whether `name` is exactly a completed attempt ID, `stage_<32 lowercase hex>`.
pub(crate) fn is_completed_attempt(name: &str) -> bool {
    name.strip_prefix(ATTEMPT_PREFIX).is_some_and(|hex| {
        hex.len() == 32
            && hex
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    })
}

fn io_error(operation: &'static str, source: io::Error) -> DraftServiceError {
    DraftServiceError::Packaging(PackagingError::Io { operation, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_exact_partial_attempt_names() {
        let hex = "0123456789abcdef0123456789abcdef";
        assert!(is_partial_attempt(&format!("stage_{hex}.partial")));
        for name in [
            format!("stage_{hex}"),
            format!("stage_{}.partial", hex.to_uppercase()),
            format!("stage_{hex}0.partial"),
            format!("draft_{hex}.partial"),
            "stage_.partial".to_owned(),
            format!("stage_{hex}.partial.bak"),
        ] {
            assert!(!is_partial_attempt(&name), "{name}");
        }
    }
}
