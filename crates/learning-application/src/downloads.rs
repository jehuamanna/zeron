//! What a learner keeps offline, and how available each lesson is.

use std::collections::BTreeSet;

use course_format::CourseManifest;

use crate::{CourseLibrary, LibraryError, torrent_import::MANIFEST_PATH};

/// Part of a course the learner asked to keep offline.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DownloadScope {
    /// Every lesson of the course.
    Course,
    /// Every lesson of one module.
    Module(String),
    /// One lesson.
    Lesson(String),
}

impl DownloadScope {
    fn stored(&self, course_id: &str) -> (&'static str, String) {
        match self {
            Self::Course => ("course", course_id.to_owned()),
            Self::Module(id) => ("module", id.clone()),
            Self::Lesson(id) => ("lesson", id.clone()),
        }
    }

    fn from_stored(scope: &str, id: String) -> Option<Self> {
        match scope {
            "course" => Some(Self::Course),
            "module" => Some(Self::Module(id)),
            "lesson" => Some(Self::Lesson(id)),
            _ => None,
        }
    }
}

/// How much of a lesson resource is usable without a network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// Nothing verified locally and not kept offline.
    OnlineOnly,
    /// Kept offline and still downloading.
    Downloading,
    /// Some verified bytes from streaming, but not kept offline.
    PartiallyAvailable,
    /// Every byte verified on this computer.
    AvailableOffline,
}

impl Availability {
    /// Label for one resource from its commitment and verified bytes.
    #[must_use]
    pub const fn of(committed: bool, verified_bytes: u64, byte_length: u64) -> Self {
        if verified_bytes >= byte_length {
            Self::AvailableOffline
        } else if committed {
            Self::Downloading
        } else if verified_bytes > 0 {
            Self::PartiallyAvailable
        } else {
            Self::OnlineOnly
        }
    }

    /// Stable name used over IPC.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OnlineOnly => "online_only",
            Self::Downloading => "downloading",
            Self::PartiallyAvailable => "partially_available",
            Self::AvailableOffline => "available_offline",
        }
    }
}

impl CourseLibrary {
    /// Keep part of a torrent course offline, or stop keeping it. Returns
    /// `false` for courses that are not torrent-sourced and for module or
    /// lesson IDs the course does not have. Already verified bytes are kept
    /// either way.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn set_download(
        &mut self,
        course_id: &str,
        scope: &DownloadScope,
        committed: bool,
    ) -> Result<bool, LibraryError> {
        if self.storage.course_torrent_info_hash(course_id)?.is_none() {
            return Ok(false);
        }
        let Some(manifest) = self.storage.course_manifest(course_id)? else {
            return Ok(false);
        };
        let known = match scope {
            DownloadScope::Course => true,
            DownloadScope::Module(id) => manifest.modules.iter().any(|module| &module.id == id),
            DownloadScope::Lesson(id) => manifest
                .modules
                .iter()
                .flat_map(|module| &module.lessons)
                .any(|lesson| &lesson.id == id),
        };
        if !known {
            return Ok(false);
        }
        let (kind, id) = scope.stored(course_id);
        self.storage
            .set_download_commitment(course_id, kind, &id, committed)?;
        Ok(true)
    }

    /// The scopes a course is kept offline by.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn downloads(&self, course_id: &str) -> Result<Vec<DownloadScope>, LibraryError> {
        Ok(self
            .storage
            .download_commitments(course_id)?
            .into_iter()
            .filter_map(|(scope, id)| DownloadScope::from_stored(&scope, id))
            .collect())
    }

    /// Resource IDs covered by a course's commitments.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn committed_resources(&self, course_id: &str) -> Result<BTreeSet<String>, LibraryError> {
        let Some(manifest) = self.storage.course_manifest(course_id)? else {
            return Ok(BTreeSet::new());
        };
        Ok(committed_resource_ids(
            &manifest,
            &self.downloads(course_id)?,
        ))
    }

    /// Torrent file indices to download in full: `course.json` plus every
    /// committed resource. `None` for courses that are not torrent-sourced.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn download_selection(&self, course_id: &str) -> Result<Option<Vec<usize>>, LibraryError> {
        let Some(source) = self
            .storage
            .torrent_course_sources()?
            .into_iter()
            .find(|source| source.course_id == course_id)
        else {
            return Ok(None);
        };
        let Ok(files) = torrent_metainfo::file_table(&source.metainfo) else {
            return Ok(None);
        };
        let committed = self.committed_resources(course_id)?;
        // course.json and its signature, which learners sharing the course
        // must serve to others importing it.
        let mut selection: BTreeSet<usize> = files
            .iter()
            .enumerate()
            .filter(|(_, file)| {
                file.path == MANIFEST_PATH || file.path == course_signing::RELEASE_SIGNATURE_FILE
            })
            .map(|(index, _)| index)
            .collect();
        selection.extend(
            self.torrent_resources(course_id)?
                .into_iter()
                .filter(|resource| committed.contains(&resource.resource_id))
                .map(|resource| resource.file_index),
        );
        Ok(Some(selection.into_iter().collect()))
    }
}

fn committed_resource_ids(manifest: &CourseManifest, scopes: &[DownloadScope]) -> BTreeSet<String> {
    let whole = scopes.contains(&DownloadScope::Course);
    manifest
        .modules
        .iter()
        .flat_map(|module| {
            let module_committed =
                whole || scopes.contains(&DownloadScope::Module(module.id.clone()));
            module.lessons.iter().filter(move |lesson| {
                module_committed || scopes.contains(&DownloadScope::Lesson(lesson.id.clone()))
            })
        })
        .flat_map(|lesson| lesson.resources.iter().map(|resource| resource.id.clone()))
        .collect()
}
