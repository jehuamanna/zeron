//! Paid lessons in the Library (ADR 0017).
//!
//! A paid course's encrypted resources stay sealed on disk and in the torrent.
//! They open only through a [`ResourceCipher`] built from the course
//! version's content key, which a [`ContentKeys`] port supplies: the
//! publisher's own credential store on the computer that published the
//! course, or the learner key opening a content key from a verified license.
//! Without a key, an encrypted resource is [`ResourceAccess::Locked`]. Keys
//! never leave the backend.

use std::collections::HashSet;

use course_access::{ContentKey, ResourceCipher};
use course_format::CourseManifest;

use crate::{CourseLibrary, LibraryError, LicensedContentKey};

/// A request for one paid course version's content key.
#[derive(Debug, Clone, Copy)]
pub struct KeyRequest<'a> {
    /// The course.
    pub course_id: &'a str,
    /// The course version.
    pub version: &'a str,
    /// Content keys sealed to learners by licenses that verified just now.
    pub licensed: &'a [LicensedContentKey],
}

/// Supplies content keys for paid course versions. It is called with the
/// Library locked and must not use the Library.
pub trait ContentKeys: Send + Sync {
    /// The content key, if this computer can produce it. Failures of the
    /// underlying stores read as "no key".
    fn content_key(&self, request: &KeyRequest<'_>) -> Option<ContentKey>;
}

/// A [`ContentKeys`] with no keys: every paid lesson stays locked.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoContentKeys;

impl ContentKeys for NoContentKeys {
    fn content_key(&self, _: &KeyRequest<'_>) -> Option<ContentKey> {
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

impl CourseLibrary {
    /// The content key of a paid course version, from `keys` and the
    /// licenses kept for the course.
    fn content_key_for(
        &self,
        manifest: &CourseManifest,
        keys: &dyn ContentKeys,
    ) -> Result<Option<ContentKey>, LibraryError> {
        let licensed = self.licensed_content_keys(&manifest.course_id, &manifest.version)?;
        Ok(keys.content_key(&KeyRequest {
            course_id: &manifest.course_id,
            version: &manifest.version,
            licensed: &licensed,
        }))
    }

    /// IDs of the course's encrypted resources that stay locked here.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn locked_resources(
        &self,
        manifest: &CourseManifest,
        keys: &dyn ContentKeys,
    ) -> Result<HashSet<String>, LibraryError> {
        if manifest.access.is_none() || self.content_key_for(manifest, keys)?.is_some() {
            return Ok(HashSet::new());
        }
        Ok(manifest
            .resources()
            .filter(|resource| resource.encryption.is_some())
            .map(|resource| resource.id.clone())
            .collect())
    }

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
        Ok(Some(self.content_key_for(&manifest, keys)?.map_or(
            ResourceAccess::Locked,
            |key| {
                ResourceAccess::Unlocked(ResourceCipher::new(
                    &key,
                    &resource.id,
                    encryption.plaintext_length,
                ))
            },
        )))
    }
}
