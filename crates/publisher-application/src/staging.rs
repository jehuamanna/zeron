use std::{fs, io, path::Path};

use course_publisher::{
    DraftCourse, PackagingCancellation, PackagingError, PackagingLimits, StagingProgress,
    stage_package,
};

use crate::{DraftServiceError, opaque_id};

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
    fs::create_dir_all(staging_root)
        .map_err(|source| io_error("creating the staging root", source))?;
    let attempt_id = opaque_id("stage")?;
    let partial = staging_root.join(format!("{attempt_id}{PARTIAL_SUFFIX}"));
    let staged = stage_package(draft, &partial, limits, cancellation, progress)
        .map_err(DraftServiceError::Packaging)?;
    if let Err(source) = fs::rename(&partial, staging_root.join(&attempt_id)) {
        let _ = fs::remove_dir_all(&partial);
        return Err(io_error("completing the staging attempt", source));
    }
    Ok(StagingReceipt {
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
/// Only real directories named exactly `stage_<32 lowercase hex>.partial`
/// directly under `staging_root` are removed. Completed attempts, symbolic
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
        let is_directory = entry
            .file_type()
            .map_err(|source| io_error("inspecting a staging attempt", source))?
            .is_dir();
        if is_directory && is_partial_attempt(&entry.file_name().to_string_lossy()) {
            fs::remove_dir_all(entry.path())
                .map_err(|source| io_error("removing an abandoned staging attempt", source))?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn is_partial_attempt(name: &str) -> bool {
    name.strip_prefix(ATTEMPT_PREFIX)
        .and_then(|rest| rest.strip_suffix(PARTIAL_SUFFIX))
        .is_some_and(|hex| {
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
