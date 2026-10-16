//! Signing releases with the publisher's identity (ADR 0014).

use std::path::Path;

use course_signing::{RELEASE_SIGNATURE_FILE, SecretKey, VerifiedRelease, sign_release};
use publisher_identity::Identity;

use crate::{
    DraftServiceError, PublisherDrafts, RELEASE_PACKAGE_DIRECTORY, RELEASE_TORRENT_FILE,
    release_directory, torrents::StagedAttempt,
};

const IDENTITY_KEY: &str = "publisher_identity";
const SIGN_RELEASES_KEY: &str = "sign_releases";

/// Signs releases during preparation. Holds the secret key only for the
/// length of one preparation.
pub struct ReleaseSigner {
    identity: Identity,
    secret: SecretKey,
    signed_at: u64,
}

impl std::fmt::Debug for ReleaseSigner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReleaseSigner")
            .field("publisher_id", &self.identity.root)
            .field("signing_key", &self.identity.current)
            .finish_non_exhaustive()
    }
}

impl ReleaseSigner {
    /// A signer for `identity` with its current secret key, signing at
    /// `signed_at` (Unix seconds).
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidAttempt`] when the key does not
    /// belong to the identity or the identity's chain is broken.
    pub fn new(
        identity: Identity,
        secret: SecretKey,
        signed_at: u64,
    ) -> Result<Self, DraftServiceError> {
        let matches = identity
            .current_key()
            .is_ok_and(|current| current == secret.public_key());
        if !matches || identity.verify().is_err() {
            return Err(DraftServiceError::InvalidAttempt(
                "the signing key does not belong to this publisher identity",
            ));
        }
        Ok(Self {
            identity,
            secret,
            signed_at,
        })
    }

    /// The identity written into `course.json` as `publisherId`.
    #[must_use]
    pub fn publisher_id(&self) -> &str {
        &self.identity.root
    }

    /// The signature file's bytes for a manifest and its digest.
    pub(crate) fn sign(
        &self,
        course_id: &str,
        course_version: &str,
        manifest_sha256: &str,
    ) -> Vec<u8> {
        sign_release(
            &self.secret,
            course_id,
            course_version,
            manifest_sha256,
            self.signed_at,
            self.identity.rotations.clone(),
        )
        .to_bytes()
    }
}

/// The signature file name, for the packaging hook.
pub(crate) const SIGNATURE_FILE: &str = RELEASE_SIGNATURE_FILE;

impl PublisherDrafts {
    /// The publisher's identity, if one was created or restored.
    ///
    /// # Errors
    /// Returns a storage failure, or [`DraftServiceError::InvalidAttempt`]
    /// for a damaged record.
    pub fn identity(&self) -> Result<Option<Identity>, DraftServiceError> {
        self.storage
            .setting(IDENTITY_KEY)?
            .map(|json| {
                serde_json::from_str(&json).map_err(|_| {
                    DraftServiceError::InvalidAttempt("the stored publisher identity is damaged")
                })
            })
            .transpose()
    }

    /// Record the publisher's identity after creating, rotating, or
    /// restoring it. The record holds public keys only.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidAttempt`] for an identity whose
    /// chain does not verify, or a storage failure.
    pub fn set_identity(&mut self, identity: &Identity) -> Result<(), DraftServiceError> {
        identity.verify().map_err(|_| {
            DraftServiceError::InvalidAttempt("the publisher identity's key chain does not verify")
        })?;
        let json = serde_json::to_string(identity)
            .map_err(|_| DraftServiceError::InvalidAttempt("the identity could not be saved"))?;
        Ok(self.storage.set_setting(IDENTITY_KEY, &json)?)
    }

    /// Whether new releases are signed when an identity exists; on unless
    /// the publisher turned it off.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn sign_releases(&self) -> Result<bool, DraftServiceError> {
        Ok(self.storage.setting(SIGN_RELEASES_KEY)?.as_deref() != Some("false"))
    }

    /// Turn signing of new releases on or off.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn set_sign_releases(&mut self, enabled: bool) -> Result<(), DraftServiceError> {
        Ok(self
            .storage
            .set_setting(SIGN_RELEASES_KEY, if enabled { "true" } else { "false" })?)
    }

    /// The verified signature of a recorded release, or `None` when it is
    /// unsigned.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidAttempt`] for unknown releases,
    /// missing files, or a package that no longer verifies.
    pub fn release_signature(
        &self,
        releases_root: &Path,
        release_id: &str,
    ) -> Result<Option<VerifiedRelease>, DraftServiceError> {
        let unknown = || DraftServiceError::InvalidAttempt("unknown release");
        let directory = release_directory(releases_root, release_id).ok_or_else(unknown)?;
        self.storage.release(release_id)?.ok_or_else(unknown)?;
        let package = StagedAttempt::open_package(
            directory.join(RELEASE_PACKAGE_DIRECTORY),
            directory.join(RELEASE_TORRENT_FILE),
        )?;
        Ok(package.signature)
    }
}
