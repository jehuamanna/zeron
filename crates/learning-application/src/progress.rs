//! Lesson and course progress for the learner.

use std::collections::HashMap;

use course_format::CourseManifest;
use learning_domain::{CourseProgress, LessonStatus, watched_enough};

use crate::{CourseLibrary, LibraryError};

/// Progress of every lesson of a course, in manifest order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseProgressView {
    /// Lesson IDs with their status.
    pub lessons: Vec<(String, LessonStatus)>,
    /// Completed lessons out of all lessons.
    pub summary: CourseProgress,
}

impl CourseLibrary {
    /// Record that a lesson resource was opened, which starts its lesson.
    /// Returns `false` for unknown courses or resources.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn record_resource_opened(
        &mut self,
        course_id: &str,
        resource_id: &str,
    ) -> Result<bool, LibraryError> {
        let Some(lesson_id) = self.lesson_of(course_id, resource_id)? else {
            return Ok(false);
        };
        self.storage.mark_lesson_started(course_id, &lesson_id)?;
        Ok(true)
    }

    /// Mark a lesson completed or not. Returns `false` for unknown courses
    /// or lessons.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn set_lesson_completed(
        &mut self,
        course_id: &str,
        lesson_id: &str,
        completed: bool,
    ) -> Result<bool, LibraryError> {
        let known = self
            .storage
            .course_manifest(course_id)?
            .is_some_and(|manifest| {
                manifest
                    .modules
                    .iter()
                    .flat_map(|module| &module.lessons)
                    .any(|lesson| lesson.id == lesson_id)
            });
        if known {
            self.storage
                .set_lesson_completed(course_id, lesson_id, completed)?;
        }
        Ok(known)
    }

    /// Complete the lesson of a resource watched far enough. Called with
    /// every saved playback position.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub(crate) fn complete_if_watched(
        &mut self,
        course_id: &str,
        resource_id: &str,
        position_seconds: f64,
        duration_seconds: f64,
    ) -> Result<(), LibraryError> {
        if !watched_enough(position_seconds, duration_seconds) {
            return Ok(());
        }
        if let Some(lesson_id) = self.lesson_of(course_id, resource_id)? {
            self.storage
                .set_lesson_completed(course_id, &lesson_id, true)?;
        }
        Ok(())
    }

    /// Progress of a course, or `None` for unknown courses.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn course_progress(
        &self,
        course_id: &str,
    ) -> Result<Option<CourseProgressView>, LibraryError> {
        let Some(manifest) = self.storage.course_manifest(course_id)? else {
            return Ok(None);
        };
        let records: HashMap<String, bool> = self
            .storage
            .lesson_progress(course_id)?
            .into_iter()
            .map(|record| (record.lesson_id, record.completed))
            .collect();
        let lessons: Vec<(String, LessonStatus)> = manifest
            .modules
            .iter()
            .flat_map(|module| &module.lessons)
            .map(|lesson| {
                let recorded = records.get(&lesson.id);
                let status = LessonStatus::from_record(
                    recorded.is_some(),
                    recorded.copied().unwrap_or(false),
                );
                (lesson.id.clone(), status)
            })
            .collect();
        let summary = CourseProgress::of(lessons.iter().map(|(_, status)| *status));
        Ok(Some(CourseProgressView { lessons, summary }))
    }

    fn lesson_of(
        &self,
        course_id: &str,
        resource_id: &str,
    ) -> Result<Option<String>, LibraryError> {
        Ok(self
            .storage
            .course_manifest(course_id)?
            .and_then(|manifest| lesson_containing(&manifest, resource_id)))
    }
}

fn lesson_containing(manifest: &CourseManifest, resource_id: &str) -> Option<String> {
    manifest
        .modules
        .iter()
        .flat_map(|module| &module.lessons)
        .find(|lesson| {
            lesson
                .resources
                .iter()
                .any(|resource| resource.id == resource_id)
        })
        .map(|lesson| lesson.id.clone())
}
