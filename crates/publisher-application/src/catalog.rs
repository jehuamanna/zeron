//! The publisher's signed catalog (ADR 0014 decision 8): releases the
//! publisher explicitly confirmed, plus the publisher's key revocations.

use std::path::Path;

use course_signing::{
    Catalog, CatalogEntry, PublicKey, RevocationStatement, SecretKey, Signed, revoke, sign_catalog,
};
use publisher_identity::Identity;
use publisher_storage::{ReleaseFiles, ReleaseRecord, ReleaseStatus};

use crate::{
    DraftServiceError, PublisherDrafts, RELEASE_PACKAGE_DIRECTORY, RELEASE_TORRENT_FILE,
    release_directory, torrents::StagedAttempt,
};

const TITLE_KEY: &str = "catalog_title";
const REVOCATIONS_KEY: &str = "issued_revocations";
const DEFAULT_TITLE: &str = "My courses";
const MAX_TITLE_CHARS: usize = 200;
const MAX_DESCRIPTION_CHARS: usize = 2_000;
const MAX_REASON_CHARS: usize = 200;

/// A release and whether the publisher confirmed it for the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogListing {
    /// The release.
    pub release: ReleaseRecord,
    /// Confirmed for the catalog.
    pub listed: bool,
}

impl PublisherDrafts {
    /// The catalog's title shown to learners.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn catalog_title(&self) -> Result<String, DraftServiceError> {
        Ok(self
            .storage
            .setting(TITLE_KEY)?
            .unwrap_or_else(|| DEFAULT_TITLE.to_owned()))
    }

    /// Set the catalog's title: 1–200 characters without control characters.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidInput`] or a storage failure.
    pub fn set_catalog_title(&mut self, title: &str) -> Result<String, DraftServiceError> {
        let title = title.trim();
        if title.is_empty()
            || title.chars().count() > MAX_TITLE_CHARS
            || title.chars().any(char::is_control)
        {
            return Err(DraftServiceError::InvalidInput {
                field: "catalogTitle".to_owned(),
                message: "must be 1–200 characters without control characters",
            });
        }
        self.storage.set_setting(TITLE_KEY, title)?;
        Ok(title.to_owned())
    }

    /// Every release with whether it is listed, newest first.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn catalog_listings(&self) -> Result<Vec<CatalogListing>, DraftServiceError> {
        let listed = self.storage.catalog_releases()?;
        Ok(self
            .storage
            .releases()?
            .into_iter()
            .map(|release| CatalogListing {
                listed: listed.contains(&release.release.release_id),
                release,
            })
            .collect())
    }

    /// Confirm a release for the catalog, or take it out. Archived releases
    /// and releases whose files were deleted cannot be listed.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidAttempt`] for unknown releases,
    /// [`DraftServiceError::ReleaseArchived`], or a storage failure.
    pub fn set_catalog_release(
        &mut self,
        release_id: &str,
        listed: bool,
    ) -> Result<(), DraftServiceError> {
        let record = self
            .storage
            .release(release_id)?
            .ok_or(DraftServiceError::InvalidAttempt("unknown release"))?;
        if listed
            && (record.status == ReleaseStatus::Archived || record.files != ReleaseFiles::Present)
        {
            return Err(DraftServiceError::ReleaseArchived);
        }
        self.storage.set_catalog_release(release_id, listed)?;
        Ok(())
    }

    /// Revocations this publisher issued, which every catalog carries.
    ///
    /// # Errors
    /// Returns a storage failure, or [`DraftServiceError::InvalidAttempt`]
    /// for a damaged record.
    pub fn issued_revocations(
        &self,
    ) -> Result<Vec<Signed<RevocationStatement>>, DraftServiceError> {
        self.storage
            .setting(REVOCATIONS_KEY)?
            .map(|json| {
                serde_json::from_str(&json).map_err(|_| {
                    DraftServiceError::InvalidAttempt("the stored revocations are damaged")
                })
            })
            .transpose()
            .map(Option::unwrap_or_default)
    }

    /// Revoke one of the identity's retired keys from `revoked_at`, signed
    /// with the current key. Learners who import a catalog then refuse
    /// anything that key signed from that time on. The current key cannot be
    /// revoked this way: rotate first.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidInput`] for a key that is not a
    /// retired key of this identity or a bad reason, or a storage failure.
    pub fn revoke_retired_key(
        &mut self,
        identity: &Identity,
        current: &SecretKey,
        key: &str,
        revoked_at: u64,
        reason: &str,
    ) -> Result<Signed<RevocationStatement>, DraftServiceError> {
        let invalid = |field: &str, message| DraftServiceError::InvalidInput {
            field: field.to_owned(),
            message,
        };
        let retired: Vec<&str> = identity
            .rotations
            .iter()
            .map(|rotation| rotation.statement.previous_key.as_str())
            .collect();
        let key: PublicKey = key
            .parse()
            .map_err(|_| invalid("key", "is not a publisher key"))?;
        if !retired.contains(&key.to_string().as_str())
            || current.public_key().to_string() != identity.current
        {
            return Err(invalid("key", "must be a retired key of this identity"));
        }
        let reason = reason.trim();
        if reason.is_empty()
            || reason.chars().count() > MAX_REASON_CHARS
            || reason.chars().any(char::is_control)
        {
            return Err(invalid("reason", "must be 1–200 characters"));
        }
        let revocation = revoke(current, &key, revoked_at, reason);
        let mut revocations = self.issued_revocations()?;
        revocations
            .retain(|existing| existing.statement.revoked_key != revocation.statement.revoked_key);
        revocations.push(revocation.clone());
        let json = serde_json::to_string(&revocations)
            .map_err(|_| DraftServiceError::InvalidAttempt("the revocation could not be saved"))?;
        self.storage.set_setting(REVOCATIONS_KEY, &json)?;
        Ok(revocation)
    }

    /// Build and sign the catalog: the confirmed releases that are not
    /// archived, each read back from its verified package, and every
    /// revocation issued.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidAttempt`] when the key does not
    /// belong to the identity or a listed package no longer verifies, or a
    /// storage failure.
    pub fn build_catalog(
        &self,
        releases_root: &Path,
        identity: &Identity,
        current: &SecretKey,
        issued_at: u64,
    ) -> Result<Catalog, DraftServiceError> {
        let root = identity
            .root_key()
            .map_err(|_| DraftServiceError::InvalidAttempt("the publisher identity is damaged"))?;
        if current.public_key().to_string() != identity.current {
            return Err(DraftServiceError::InvalidAttempt(
                "the signing key does not belong to this publisher identity",
            ));
        }
        let mut entries = Vec::new();
        for listing in self.catalog_listings()? {
            let record = listing.release;
            if !listing.listed || record.status == ReleaseStatus::Archived {
                continue;
            }
            let directory = release_directory(releases_root, &record.release.release_id)
                .ok_or(DraftServiceError::InvalidAttempt("unknown release"))?;
            let package = StagedAttempt::open_package(
                directory.join(RELEASE_PACKAGE_DIRECTORY),
                directory.join(RELEASE_TORRENT_FILE),
            )?;
            entries.push(CatalogEntry {
                course_id: record.release.course_id,
                course_version: record.release.course_version,
                title: record.release.title,
                description: package
                    .manifest
                    .description
                    .unwrap_or_default()
                    .chars()
                    .filter(|character| !character.is_control() || *character == '\n')
                    .take(MAX_DESCRIPTION_CHARS)
                    .collect(),
                magnet: record.release.magnet,
                info_hash: record.release.info_hash,
                manifest_sha256: record.release.manifest_sha256,
            });
        }
        Ok(sign_catalog(
            current,
            &root,
            &self.catalog_title()?,
            issued_at,
            entries,
            self.issued_revocations()?,
            identity.rotations.clone(),
        ))
    }
}
