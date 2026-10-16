//! Licensing a learner by hand (ADR 0017 decision 4): the publisher seals
//! every released paid version's content key to the learner's key and signs
//! the license with its own signing key, so the Library verifies it against
//! the course's pinned publisher with no issuer and no network.

use course_access::wrap_content_key;
use course_signing::{LicenseFile, LicensedKey, PublicKey, license_file, sign_license};
use publisher_identity::CredentialStore;

use crate::{DraftServiceError, PublisherDrafts, ReleaseSigner, stored_content_key};

impl PublisherDrafts {
    /// Every version of `course_id` with a release record, oldest first.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn released_versions(&self, course_id: &str) -> Result<Vec<String>, DraftServiceError> {
        let mut versions: Vec<String> = Vec::new();
        for record in self.releases()? {
            let version = record.release.course_version;
            if record.release.course_id == course_id && !versions.contains(&version) {
                versions.push(version);
            }
        }
        Ok(versions)
    }
}

impl ReleaseSigner {
    /// A license for `learner_key`, signed with the publisher's current key
    /// and carrying its rotation chain.
    #[must_use]
    pub fn license(
        &self,
        course_id: &str,
        learner_key: &PublicKey,
        keys: Vec<LicensedKey>,
    ) -> LicenseFile {
        let root: PublicKey = self
            .publisher_id()
            .parse()
            .unwrap_or_else(|_| unreachable!("a signer's identity verified when it was made"));
        license_file(
            &root,
            self.rotations(),
            None,
            sign_license(
                self.secret(),
                course_id,
                learner_key,
                keys,
                self.signed_at(),
            ),
        )
    }
}

/// A license unlocking every version in `versions` whose content key this
/// computer's keychain holds, for the learner with the given keys.
///
/// # Errors
/// Returns [`DraftServiceError::InvalidAttempt`] when the keychain cannot be
/// read, no version has a content key here, or the learner's encryption key
/// is unusable.
pub fn issue_license(
    store: &dyn CredentialStore,
    signer: &ReleaseSigner,
    course_id: &str,
    versions: &[String],
    learner_key: &PublicKey,
    learner_encryption_key: &[u8; 32],
) -> Result<LicenseFile, DraftServiceError> {
    let mut keys = Vec::new();
    for version in versions {
        let key = stored_content_key(store, course_id, version)
            .map_err(|_| DraftServiceError::InvalidAttempt("the keychain could not be read"))?;
        if let Some(key) = key {
            let wrapped = wrap_content_key(&key, learner_encryption_key, course_id, version)
                .map_err(|_| DraftServiceError::InvalidAttempt("the learner ID is not usable"))?;
            keys.push(LicensedKey {
                course_version: version.clone(),
                wrapped_content_key: wrapped,
            });
        }
    }
    if keys.is_empty() {
        return Err(DraftServiceError::InvalidAttempt(
            "no paid release of this course has its content key on this computer",
        ));
    }
    Ok(signer.license(course_id, learner_key, keys))
}
