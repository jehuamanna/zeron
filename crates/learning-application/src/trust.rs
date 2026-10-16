//! Who published a course: verifying release signatures and pinning each
//! course to its publisher on first use (ADR 0014 decision 6).

use course_format::CourseManifest;
use course_signing::{fingerprint, parse_release_signature, verify_release};
use learning_storage::{CourseSignature, CourseTrust};

use crate::{CourseLibrary, LibraryError};

/// Who published a course, as far as this library can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoursePublisher {
    /// The verified signature of the current version, if it is signed.
    pub signature: Option<CourseSignature>,
    /// The publisher the course is pinned to, if any.
    pub pinned: Option<String>,
}

impl CoursePublisher {
    /// The fingerprint people compare, of the signing identity or the pin.
    #[must_use]
    pub fn fingerprint(&self) -> Option<String> {
        self.signature
            .as_ref()
            .map(|signature| signature.publisher_id.as_str())
            .or(self.pinned.as_deref())
            .and_then(|id| id.parse().ok())
            .map(|key| fingerprint(&key))
    }
}

impl CourseLibrary {
    /// Decide whether a course version may be saved, and what to record.
    ///
    /// - A signature present must verify against the manifest and its
    ///   `publisherId`, and its key must not be revoked.
    /// - A course pinned to a publisher accepts only versions signed by that
    ///   publisher, so nobody else can replace it.
    /// - The first signed version pins the course.
    ///
    /// # Errors
    /// Returns [`LibraryError::Signature`] or [`LibraryError::PublisherChanged`].
    pub(crate) fn course_trust(
        &self,
        manifest: &CourseManifest,
        manifest_sha256: &str,
        signature: Option<&[u8]>,
    ) -> Result<CourseTrust, LibraryError> {
        let verified = signature
            .map(|bytes| {
                let parsed = parse_release_signature(bytes)
                    .map_err(|error| LibraryError::Signature(error.to_string()))?;
                let publisher_id = manifest.publisher.publisher_id.as_deref().ok_or_else(|| {
                    LibraryError::Signature(
                        "the course is signed but does not name its publisher".to_owned(),
                    )
                })?;
                let verified = verify_release(
                    &parsed,
                    &manifest.course_id,
                    &manifest.version,
                    manifest_sha256,
                    publisher_id,
                    &self.revocations()?,
                )
                .map_err(|error| LibraryError::Signature(error.to_string()))?;
                if let Some(reason) = verified.revoked {
                    return Err(LibraryError::Signature(format!(
                        "it was signed with a key its publisher revoked ({reason})"
                    )));
                }
                Ok(CourseSignature {
                    publisher_id: verified.root.to_string(),
                    signing_key: verified.signing_key.to_string(),
                    signed_at: verified.signed_at,
                })
            })
            .transpose()?;
        let pinned = self.storage.course_pin(&manifest.course_id)?;
        match (&pinned, &verified) {
            (Some(pinned), Some(signature)) if *pinned == signature.publisher_id => {}
            (Some(pinned), found) => {
                return Err(LibraryError::PublisherChanged {
                    pinned: fingerprint_of(pinned),
                    found: found
                        .as_ref()
                        .map(|signature| fingerprint_of(&signature.publisher_id)),
                });
            }
            (None, _) => {}
        }
        Ok(CourseTrust {
            pin: verified
                .as_ref()
                .map(|signature| signature.publisher_id.clone()),
            signature: verified,
        })
    }

    /// Key revocations learned from catalogs. Statements that no longer
    /// parse are skipped; verification decides whether each one counts.
    pub(crate) fn revocations(
        &self,
    ) -> Result<Vec<course_signing::RevocationRecord>, LibraryError> {
        Ok(self
            .storage
            .revocation_statements()?
            .iter()
            .filter_map(|json| serde_json::from_str(json).ok())
            .collect())
    }

    /// Keep a signed key revocation with the rotation chain it arrived with,
    /// for example from a catalog. Whether it counts is decided when a
    /// signature is checked: only revocations made by the revoked key or a
    /// later key of the same publisher's chain apply.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn add_key_revocation(
        &mut self,
        revocation: &course_signing::RevocationRecord,
    ) -> Result<(), LibraryError> {
        let json = serde_json::to_string(revocation)
            .map_err(|error| LibraryError::Signature(error.to_string()))?;
        Ok(self.storage.add_revocation_statement(&json)?)
    }

    /// Who published a course.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn course_publisher(&self, course_id: &str) -> Result<CoursePublisher, LibraryError> {
        Ok(CoursePublisher {
            signature: self.storage.course_signature(course_id)?,
            pinned: self.storage.course_pin(course_id)?,
        })
    }

    /// Stop requiring a course's pinned publisher. The next signed version
    /// imported pins the course again, whoever signed it. Notes and progress
    /// are untouched. Returns whether there was a pin.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn forget_course_publisher(&mut self, course_id: &str) -> Result<bool, LibraryError> {
        Ok(self.storage.forget_course_pin(course_id)?)
    }
}

fn fingerprint_of(publisher_id: &str) -> String {
    publisher_id.parse().map_or_else(
        |_| "an unknown publisher".to_owned(),
        |key| fingerprint(&key),
    )
}
