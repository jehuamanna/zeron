//! Free disk space checks before release preparation copies course files.

use std::path::Path;

use crate::DraftServiceError;

/// Free space a release preparation must leave on the staging disk.
pub const DISK_RESERVE_BYTES: u64 = 1024 * 1024 * 1024;

/// Bytes available to this user on the filesystem holding `path` (or its
/// nearest existing ancestor), or `None` where the platform cannot tell.
#[must_use]
pub fn available_bytes(path: &Path) -> Option<u64> {
    let path = path.ancestors().find(|candidate| candidate.exists())?;
    #[cfg(unix)]
    #[allow(
        clippy::useless_conversion,
        reason = "the field types differ between platforms"
    )]
    {
        let stats = nix::sys::statvfs::statvfs(path).ok()?;
        u64::from(stats.blocks_available()).checked_mul(u64::from(stats.fragment_size()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// Refuse when `needed` bytes would not fit while keeping the reserve free.
/// Unknown free space never refuses.
///
/// # Errors
/// Returns [`DraftServiceError::InsufficientSpace`].
pub fn check_space(available: Option<u64>, needed: u64) -> Result<(), DraftServiceError> {
    match available {
        Some(available) if available < needed.saturating_add(DISK_RESERVE_BYTES) => {
            Err(DraftServiceError::InsufficientSpace {
                needed_bytes: needed,
                available_bytes: available,
            })
        }
        _ => Ok(()),
    }
}
