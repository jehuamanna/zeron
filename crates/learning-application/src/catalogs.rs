//! Publisher catalogs in the Library (ADR 0014 decision 8): verified lists
//! of releases to import, and the carrier of key revocations. A catalog
//! entry is only a pointer; the release's own signature is what is trusted.

use course_signing::{CatalogEntry, fingerprint, parse_catalog, verify_catalog};
use learning_storage::StoredCatalog;

use crate::{CourseLibrary, LibraryError};

/// Longest HTTPS address kept for refreshing a catalog.
pub const MAX_CATALOG_URL_BYTES: usize = 2_048;

/// How a catalog entry relates to the Library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryStatus {
    /// The course is not in the Library.
    Available,
    /// This exact version is in the Library.
    InLibrary,
    /// A newer version of a course the Library has from this publisher.
    Update,
    /// Another version of a course the Library has; not newer, or its
    /// version is not comparable.
    OtherVersion,
    /// The Library has this course from another publisher.
    OtherPublisher,
}

impl EntryStatus {
    /// Stable machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::InLibrary => "in_library",
            Self::Update => "update",
            Self::OtherVersion => "other_version",
            Self::OtherPublisher => "other_publisher",
        }
    }
}

/// One entry of a catalog, with its relation to the Library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntryView {
    /// The entry as the publisher listed it.
    pub entry: CatalogEntry,
    /// How it relates to the Library.
    pub status: EntryStatus,
}

/// A kept catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogView {
    /// The publisher's identity.
    pub publisher_id: String,
    /// Its fingerprint, for comparing out of band.
    pub fingerprint: String,
    /// The catalog's title.
    pub title: String,
    /// When the publisher issued it.
    pub issued_at: u64,
    /// HTTPS address it is refreshed from, if any.
    pub source_url: Option<String>,
    /// Listed releases.
    pub entries: Vec<CatalogEntryView>,
}

impl CourseLibrary {
    /// Verify and keep a catalog, replacing an older one of the same
    /// publisher, and keep the revocations it carries.
    ///
    /// # Errors
    /// Returns [`LibraryError::Signature`] for a catalog that does not
    /// verify, is signed with a revoked key, or is older than the one kept,
    /// or a storage failure.
    pub fn import_catalog(
        &mut self,
        bytes: &[u8],
        source_url: Option<&str>,
    ) -> Result<CatalogView, LibraryError> {
        let refused = |message: String| LibraryError::Signature(message);
        let catalog = parse_catalog(bytes).map_err(|error| refused(error.to_string()))?;
        let verified = verify_catalog(&catalog).map_err(|error| refused(error.to_string()))?;
        if let Some(reason) = verified.revoked {
            return Err(refused(format!(
                "the catalog was signed with a key its publisher revoked ({reason})"
            )));
        }
        let publisher_id = verified.root.to_string();
        let existing = self
            .storage
            .catalogs()?
            .into_iter()
            .find(|kept| kept.publisher_id == publisher_id);
        if existing
            .as_ref()
            .is_some_and(|kept| kept.issued_at > verified.issued_at)
        {
            return Err(refused(
                "this catalog is older than the one the Library already has".to_owned(),
            ));
        }
        for revocation in &catalog.statement.revocations {
            self.add_key_revocation(&course_signing::RevocationRecord {
                publisher_id: publisher_id.clone(),
                rotations: catalog.rotations.clone(),
                revocation: revocation.clone(),
            })?;
        }
        let source_url = source_url
            .map(str::to_owned)
            .or_else(|| existing.and_then(|kept| kept.source_url));
        self.storage.save_catalog(&StoredCatalog {
            publisher_id: publisher_id.clone(),
            title: catalog.statement.title.clone(),
            issued_at: verified.issued_at,
            source_url,
            document: bytes.to_vec(),
        })?;
        self.catalogs()?
            .into_iter()
            .find(|view| view.publisher_id == publisher_id)
            .ok_or_else(|| refused("the catalog could not be read back".to_owned()))
    }

    /// Every kept catalog, with each entry related to the Library.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn catalogs(&self) -> Result<Vec<CatalogView>, LibraryError> {
        let courses = self.storage.list_courses()?;
        let mut views = Vec::new();
        for kept in self.storage.catalogs()? {
            // Kept catalogs verified when imported; skip one damaged since.
            let Ok(catalog) = parse_catalog(&kept.document) else {
                continue;
            };
            let mut entries = Vec::new();
            for entry in catalog.statement.entries {
                let status = match courses
                    .iter()
                    .find(|course| course.course_id == entry.course_id)
                {
                    None => EntryStatus::Available,
                    Some(course) if course.course_version == entry.course_version => {
                        EntryStatus::InLibrary
                    }
                    Some(course) => {
                        let pinned = self.storage.course_pin(&course.course_id)?;
                        if pinned
                            .as_deref()
                            .is_some_and(|pinned| pinned != kept.publisher_id)
                        {
                            EntryStatus::OtherPublisher
                        } else if newer(&entry.course_version, &course.course_version) {
                            EntryStatus::Update
                        } else {
                            EntryStatus::OtherVersion
                        }
                    }
                };
                entries.push(CatalogEntryView { entry, status });
            }
            views.push(CatalogView {
                fingerprint: kept
                    .publisher_id
                    .parse()
                    .map(|key| fingerprint(&key))
                    .unwrap_or_default(),
                publisher_id: kept.publisher_id,
                title: kept.title,
                issued_at: kept.issued_at,
                source_url: kept.source_url,
                entries,
            });
        }
        Ok(views)
    }

    /// The HTTPS address a kept catalog refreshes from.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn catalog_source(&self, publisher_id: &str) -> Result<Option<String>, LibraryError> {
        Ok(self
            .storage
            .catalogs()?
            .into_iter()
            .find(|kept| kept.publisher_id == publisher_id)
            .and_then(|kept| kept.source_url))
    }

    /// Forget a catalog. Courses imported from it and the revocations it
    /// brought stay. Returns whether one was kept.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn remove_catalog(&mut self, publisher_id: &str) -> Result<bool, LibraryError> {
        Ok(self.storage.remove_catalog(publisher_id)?)
    }
}

fn newer(candidate: &str, current: &str) -> bool {
    match (
        semver::Version::parse(candidate),
        semver::Version::parse(current),
    ) {
        (Ok(candidate), Ok(current)) => candidate > current,
        _ => false,
    }
}
