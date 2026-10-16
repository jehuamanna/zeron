//! Paid lessons in the Library (ADR 0017).
//!
//! A paid course's encrypted resources stay sealed on disk and in the torrent.
//! They open only through a [`ResourceCipher`] built from the course
//! version's content key, which a [`ContentKeys`] port supplies: today the
//! publisher's own credential store on the computer that published the
//! course, later a learner's verified licenses. Without a key, an encrypted
//! resource is [`ResourceAccess::Locked`]. Keys never leave the backend.

use std::collections::HashSet;

use course_access::{ContentKey, ResourceCipher};
use course_format::CourseManifest;

use crate::{CourseLibrary, LibraryError};

/// Supplies content keys for paid course versions.
pub trait ContentKeys: Send + Sync {
    /// The content key of `course_id` at `version`, if this computer holds
    /// one. Failures of the underlying store read as "no key".
    fn content_key(&self, course_id: &str, version: &str) -> Option<ContentKey>;
}

/// A [`ContentKeys`] with no keys: every paid lesson stays locked.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoContentKeys;

impl ContentKeys for NoContentKeys {
    fn content_key(&self, _: &str, _: &str) -> Option<ContentKey> {
        None
    }
}

/// Whether and how a resource's bytes can be read.
#[derive(Debug, Clone)]
pub enum ResourceAccess {
    /// Not encrypted: read the package bytes as they are.
    Free,
    /// Encrypted, and this cipher opens it.
    Unlocked(ResourceCipher),
    /// Encrypted, and no content key is available.
    Locked,
}

impl ResourceAccess {
    /// Whether the resource cannot be read.
    #[must_use]
    pub const fn is_locked(&self) -> bool {
        matches!(self, Self::Locked)
    }
}

/// Whether the course version's paid lessons open on this computer.
#[must_use]
pub fn paid_content_unlocked(manifest: &CourseManifest, keys: &dyn ContentKeys) -> bool {
    manifest.access.is_none()
        || keys
            .content_key(&manifest.course_id, &manifest.version)
            .is_some()
}

/// IDs of the course's encrypted resources that stay locked here.
#[must_use]
pub fn locked_resources(manifest: &CourseManifest, keys: &dyn ContentKeys) -> HashSet<String> {
    if paid_content_unlocked(manifest, keys) {
        return HashSet::new();
    }
    manifest
        .resources()
        .filter(|resource| resource.encryption.is_some())
        .map(|resource| resource.id.clone())
        .collect()
}

impl CourseLibrary {
    /// How a resource of a course opens here. `None` for unknown courses or
    /// resources.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn resource_access(
        &self,
        course_id: &str,
        resource_id: &str,
        keys: &dyn ContentKeys,
    ) -> Result<Option<ResourceAccess>, LibraryError> {
        let Some(manifest) = self.manifest(course_id)? else {
            return Ok(None);
        };
        let Some(resource) = manifest
            .resources()
            .find(|resource| resource.id == resource_id)
        else {
            return Ok(None);
        };
        let Some(encryption) = &resource.encryption else {
            return Ok(Some(ResourceAccess::Free));
        };
        Ok(Some(
            keys.content_key(&manifest.course_id, &manifest.version)
                .map_or(ResourceAccess::Locked, |key| {
                    ResourceAccess::Unlocked(ResourceCipher::new(
                        &key,
                        &resource.id,
                        encryption.plaintext_length,
                    ))
                }),
        ))
    }
}
