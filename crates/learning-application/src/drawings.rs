//! Excalidraw drawings kept with a lesson.

use std::collections::BTreeSet;

use learning_domain::{DrawingError, DrawingFile, MAX_FILES_PER_DRAWING, Scene};
use learning_storage::DrawingWrite;

use crate::{CourseLibrary, LibraryError, notes::check_target};

/// A lesson's drawing and every image placed in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawing {
    /// Serialized Excalidraw scene.
    pub scene_json: String,
    /// Increases with every save; saves must name it.
    pub revision: u64,
    /// Embedded images.
    pub files: Vec<DrawingFile>,
}

/// Outcome of saving a drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawingSave {
    /// Saved at this revision.
    Saved {
        /// The new revision.
        revision: u64,
    },
    /// The drawing changed since the caller read it; nothing was overwritten.
    Conflict {
        /// Revision to reload.
        current_revision: u64,
    },
}

impl CourseLibrary {
    /// The drawing of a lesson, if the learner made one.
    ///
    /// # Errors
    ///
    /// Returns a storage failure, or [`LibraryError::Drawing`] if a stored
    /// image no longer passes validation.
    pub fn drawing(
        &self,
        course_id: &str,
        lesson_id: &str,
    ) -> Result<Option<Drawing>, LibraryError> {
        let Some(record) = self.storage.drawing(course_id, lesson_id)? else {
            return Ok(None);
        };
        let files = record
            .files
            .iter()
            .map(|(id, mime_type, data_url)| DrawingFile::new(id, mime_type, data_url))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(Drawing {
            scene_json: record.scene_json,
            revision: record.revision,
            files,
        }))
    }

    /// Lessons of a course that have a drawing, in lesson-ID order. Drawings
    /// of lessons a newer course version dropped are listed too; the caller
    /// decides how to show them.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn drawn_lessons(&self, course_id: &str) -> Result<Vec<String>, LibraryError> {
        Ok(self.storage.drawing_lessons(course_id)?)
    }

    /// Save a lesson's drawing: create it when `expected_revision` is `None`,
    /// otherwise replace it only if it is still at that revision. Images whose
    /// IDs are already stored are kept unchanged; new ones are added.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Drawing`] for an invalid scene or image or too
    /// many images, [`LibraryError::NoteTarget`] for an unknown course or
    /// lesson, or a storage failure.
    pub fn save_drawing(
        &mut self,
        course_id: &str,
        lesson_id: &str,
        expected_revision: Option<u64>,
        scene_json: &str,
        files: &[DrawingFile],
    ) -> Result<DrawingSave, LibraryError> {
        let scene = Scene::parse(scene_json)?;
        let manifest = self
            .storage
            .course_manifest(course_id)?
            .ok_or(LibraryError::NoteTarget("the course is not in the library"))?;
        check_target(&manifest, lesson_id, None, None)?;
        let stored: BTreeSet<String> = self
            .storage
            .drawing(course_id, lesson_id)?
            .map(|record| record.files.into_iter().map(|(id, _, _)| id).collect())
            .unwrap_or_default();
        let mut new_files = Vec::new();
        let mut ids = stored.clone();
        for file in files {
            let file = DrawingFile::new(&file.id, &file.mime_type, &file.data_url)?;
            if ids.insert(file.id.clone()) {
                new_files.push((file.id, file.mime_type, file.data_url));
            }
        }
        if ids.len() > MAX_FILES_PER_DRAWING {
            return Err(DrawingError::InvalidFile("the drawing holds too many images").into());
        }
        Ok(
            match self.storage.save_drawing(
                course_id,
                lesson_id,
                expected_revision,
                &scene.json,
                &new_files,
            )? {
                DrawingWrite::Saved { revision } => DrawingSave::Saved { revision },
                DrawingWrite::Conflict { current_revision } => {
                    DrawingSave::Conflict { current_revision }
                }
            },
        )
    }
}
