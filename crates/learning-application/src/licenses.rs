//! Licenses for paid courses (ADR 0017 decision 4), verified offline.
//!
//! A license is checked against the publisher the course is pinned to and
//! the key revocations the Library knows, when it is imported and again
//! whenever it is used, so a later revocation takes effect. Licenses are
//! kept as the files they came in, as learner data.

use course_signing::{LicensedKey, PublicKey, parse_license, verify_license};
use sha2::{Digest, Sha256};

use crate::{CourseLibrary, LibraryError};

/// A license that was imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedLicense {
    /// The licensed course.
    pub course_id: String,
    /// Its title in the Library.
    pub course_title: String,
    /// The course versions the license unlocks.
    pub versions: Vec<String>,
    /// Whether it unlocks the version in the Library now.
    pub unlocks_current: bool,
    /// `false` when the same license file was imported before.
    pub new: bool,
}

/// A content key a verified license seals to a learner key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicensedContentKey {
    /// The learner signing key (`ed25519-…`) the license is for.
    pub learner_key: String,
    /// The content key sealed to that learner's encryption key.
    pub wrapped_content_key: String,
}

impl CourseLibrary {
    /// Verify a license file for `learner_key` and keep it.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::License`] when the file is malformed, names a
    /// course that is not in the Library or has no paid lessons, the course
    /// is not signed, or the license does not verify for this publisher and
    /// learner; or a storage failure.
    pub fn import_license(
        &mut self,
        bytes: &[u8],
        learner_key: &PublicKey,
    ) -> Result<ImportedLicense, LibraryError> {
        let refused = |message: &str| LibraryError::License(message.to_owned());
        let file =
            parse_license(bytes).map_err(|error| LibraryError::License(error.to_string()))?;
        let course_id = file.license.statement.course_id.clone();
        let manifest = self
            .manifest(&course_id)?
            .ok_or_else(|| refused("its course is not in your library; import the course first"))?;
        if manifest.access.is_none() {
            return Err(refused("its course has no paid lessons"));
        }
        let pinned = self.storage.course_pin(&course_id)?.ok_or_else(|| {
            refused("its course is not signed by its publisher, so the license cannot be checked")
        })?;
        let verified = verify_license(&file, &pinned, learner_key, &self.revocations()?)
            .map_err(|error| LibraryError::License(error.to_string()))?;
        if let Some(reason) = verified.revoked {
            return Err(LibraryError::License(format!(
                "it was signed with a key its publisher revoked ({reason})"
            )));
        }
        let digest = Sha256::digest(bytes);
        let sha256 = digest
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                use std::fmt::Write as _;
                let _ = write!(hex, "{byte:02x}");
                hex
            });
        let new = self.storage.add_license(
            &sha256,
            &course_id,
            &learner_key.to_string(),
            verified.issued_at,
            bytes,
        )?;
        let versions: Vec<String> = verified
            .keys
            .iter()
            .map(|key| key.course_version.clone())
            .collect();
        Ok(ImportedLicense {
            unlocks_current: versions.contains(&manifest.version),
            course_title: manifest.title,
            course_id,
            versions,
            new,
        })
    }

    /// Content keys for `course_id` at `version` from the licenses kept for
    /// it, each verified again now against the pinned publisher and known
    /// revocations. Licenses that no longer verify are skipped.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn licensed_content_keys(
        &self,
        course_id: &str,
        version: &str,
    ) -> Result<Vec<LicensedContentKey>, LibraryError> {
        let documents = self.storage.licenses(course_id)?;
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let Some(pinned) = self.storage.course_pin(course_id)? else {
            return Ok(Vec::new());
        };
        let revocations = self.revocations()?;
        Ok(documents
            .iter()
            .filter_map(|bytes| {
                let file = parse_license(bytes).ok()?;
                let statement = &file.license.statement;
                let learner_key: PublicKey = statement.learner_key.parse().ok()?;
                let verified = verify_license(&file, &pinned, &learner_key, &revocations).ok()?;
                if verified.revoked.is_some() || verified.course_id != course_id {
                    return None;
                }
                verified
                    .keys
                    .into_iter()
                    .find(|key: &LicensedKey| key.course_version == version)
                    .map(|key| LicensedContentKey {
                        learner_key: learner_key.to_string(),
                        wrapped_content_key: key.wrapped_content_key,
                    })
            })
            .collect())
    }
}
