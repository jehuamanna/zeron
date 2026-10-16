//! Deleting a whole course from the Publish side (ADR 0016).
//!
//! Every draft goes, every release is archived, taken out of the catalog,
//! and loses its files, and the course leaves the course list. The release
//! rows stay hidden, so a version is never reused with other content.

use std::path::Path;

use publisher_storage::{ReleaseFiles, ReleaseStatus};

use crate::{ActivityKind, DraftServiceError, PublisherDrafts};

/// The name used when a course has no title, as the course list shows it.
pub const UNTITLED_COURSE: &str = "Untitled course";

/// What deleting a course removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseDeletion {
    /// Drafts deleted.
    pub drafts: usize,
    /// Releases whose files were deleted now.
    pub releases: usize,
}

impl PublisherDrafts {
    /// The title the course list shows for a course: its most recently
    /// edited draft's, else its newest release's. `None` for an unknown or
    /// already deleted course.
    ///
    /// # Errors
    /// Storage failures.
    pub fn course_title(&self, course_id: &str) -> Result<Option<String>, DraftServiceError> {
        if self
            .storage
            .hidden_courses()?
            .iter()
            .any(|hidden| hidden == course_id)
        {
            return Ok(None);
        }
        let draft = self
            .storage
            .list_drafts()?
            .into_iter()
            .find(|draft| draft.course_id == course_id);
        let title = match draft {
            Some(draft) => Some(draft.title),
            None => self
                .storage
                .releases()?
                .into_iter()
                .find(|record| record.release.course_id == course_id)
                .map(|record| record.release.title),
        };
        Ok(title.map(|title| {
            if title.trim().is_empty() {
                UNTITLED_COURSE.to_owned()
            } else {
                title
            }
        }))
    }

    /// Courses deleted from the course list; their drafts and releases are
    /// left out of it.
    ///
    /// # Errors
    /// Storage failures.
    pub fn hidden_courses(&self) -> Result<Vec<String>, DraftServiceError> {
        Ok(self.storage.hidden_courses()?)
    }

    /// Releases of a course that are still published or superseded, whose
    /// seeding the caller stops before [`Self::delete_course`].
    ///
    /// # Errors
    /// Storage failures.
    pub fn course_releases_in_use(
        &self,
        course_id: &str,
    ) -> Result<Vec<String>, DraftServiceError> {
        Ok(self
            .storage
            .releases()?
            .into_iter()
            .filter(|record| {
                record.release.course_id == course_id && record.status != ReleaseStatus::Archived
            })
            .map(|record| record.release.release_id)
            .collect())
    }

    /// Draft IDs of a course, so the caller can refuse while one is being
    /// prepared for publishing.
    ///
    /// # Errors
    /// Storage failures.
    pub fn course_draft_ids(&self, course_id: &str) -> Result<Vec<String>, DraftServiceError> {
        Ok(self
            .storage
            .list_drafts()?
            .into_iter()
            .filter(|draft| draft.course_id == course_id)
            .map(|draft| draft.draft_id)
            .collect())
    }

    /// Delete a whole course. `confirmation` must be the course's title as
    /// the course list shows it; the check runs here so a stray call cannot
    /// delete anything. Seeding must already be stopped by the caller; any
    /// release still in use is archived here. Release files are deleted with
    /// the same crash-safe steps as one release's (ADR 0006). Deleting a
    /// deleted course does nothing.
    ///
    /// # Errors
    /// [`DraftServiceError::InvalidInput`] for a wrong confirmation,
    /// [`DraftServiceError::InvalidAttempt`] for an unknown course, or a
    /// storage or I/O failure. A failure part-way leaves the course listed
    /// with what remains; running it again finishes it.
    pub fn delete_course(
        &mut self,
        releases_root: &Path,
        course_id: &str,
        confirmation: &str,
    ) -> Result<CourseDeletion, DraftServiceError> {
        let Some(title) = self.course_title(course_id)? else {
            return Err(DraftServiceError::InvalidAttempt("unknown course"));
        };
        if confirmation.trim() != title.trim() {
            return Err(DraftServiceError::InvalidInput {
                field: "confirmation".to_owned(),
                message: "must match the course title",
            });
        }
        let mut report = CourseDeletion {
            drafts: 0,
            releases: 0,
        };
        for draft in self
            .storage
            .list_drafts()?
            .into_iter()
            .filter(|draft| draft.course_id == course_id)
        {
            if self.storage.delete_draft(&draft.draft_id, draft.revision)? {
                report.drafts += 1;
            }
        }
        let releases: Vec<_> = self
            .storage
            .releases()?
            .into_iter()
            .filter(|record| record.release.course_id == course_id)
            .collect();
        for record in releases {
            let release_id = record.release.release_id.clone();
            let record = if record.status == ReleaseStatus::Archived {
                record
            } else {
                self.archive_release(&release_id)?
            };
            self.storage.set_catalog_release(&release_id, false)?;
            if record.files != ReleaseFiles::Deleted {
                self.delete_archived_files(releases_root, &record)?;
                report.releases += 1;
            }
        }
        self.storage.hide_course(course_id)?;
        self.record_activity(
            None,
            ActivityKind::CourseDeleted,
            &format!(
                "Course “{title}” deleted: {} draft(s), and the files of {} release(s)",
                report.drafts, report.releases
            ),
        );
        Ok(report)
    }
}
