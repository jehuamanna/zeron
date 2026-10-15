//! Learner notes anchored to lessons, video moments, and PDF pages.

use std::fmt::Write as _;

use course_format::{CourseManifest, ResourceKind};
use learning_domain::{Anchor, NoteBody};
use learning_storage::{NoteRecord, NoteWrite};

use crate::{CourseLibrary, LibraryError};

/// A learner note as the application sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// Opaque note ID.
    pub note_id: String,
    /// Lesson the note belongs to.
    pub lesson_id: String,
    /// Where in the lesson it points.
    pub anchor: Anchor,
    /// The Tiptap document.
    pub body_json: String,
    /// Plain text of the document.
    pub text: String,
    /// Increases with every saved change; writes must name it.
    pub revision: u64,
    /// Creation time in Unix seconds.
    pub created_at: u64,
    /// Last change in Unix seconds.
    pub updated_at: u64,
}

/// Outcome of changing an existing note.
#[derive(Debug, Clone, PartialEq)]
pub enum NoteChange {
    /// The change was saved; for a delete, the note as it was.
    Saved(Note),
    /// The note changed since the caller read it; nothing was overwritten.
    Conflict {
        /// Revision to reload.
        current_revision: u64,
    },
    /// No such note.
    NotFound,
}

impl CourseLibrary {
    /// Create a note in a lesson. The anchor must point at a video (for a
    /// timestamp) or PDF (for a page) resource of that lesson.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Note`] for an invalid anchor or document,
    /// [`LibraryError::NoteTarget`] for an unknown lesson or resource, or a
    /// storage failure.
    pub fn create_note(
        &mut self,
        course_id: &str,
        lesson_id: &str,
        anchor: &Anchor,
        body_json: &str,
    ) -> Result<Note, LibraryError> {
        let body = NoteBody::parse(body_json)?;
        self.check_anchor(course_id, lesson_id, anchor)?;
        let anchor_json = anchor.to_json();
        let record = self.storage.insert_note(
            &note_id()?,
            course_id,
            lesson_id,
            &anchor_json,
            &body.json,
            &body.text,
        )?;
        note(record)
    }

    /// Replace a note's anchor and document if it is still at
    /// `expected_revision`.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Note`], [`LibraryError::NoteTarget`], or a
    /// storage failure.
    pub fn update_note(
        &mut self,
        note_id: &str,
        expected_revision: u64,
        anchor: &Anchor,
        body_json: &str,
    ) -> Result<NoteChange, LibraryError> {
        let body = NoteBody::parse(body_json)?;
        let Some(existing) = self.storage.note(note_id)? else {
            return Ok(NoteChange::NotFound);
        };
        self.check_anchor(&existing.course_id, &existing.lesson_id, anchor)?;
        let anchor_json = anchor.to_json();
        change(self.storage.update_note(
            note_id,
            expected_revision,
            &anchor_json,
            &body.json,
            &body.text,
        )?)
    }

    /// Delete a note if it is still at `expected_revision`.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn delete_note(
        &mut self,
        note_id: &str,
        expected_revision: u64,
    ) -> Result<NoteChange, LibraryError> {
        change(self.storage.delete_note(note_id, expected_revision)?)
    }

    /// Notes of a course, or of one lesson, oldest first.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn notes(
        &self,
        course_id: &str,
        lesson_id: Option<&str>,
    ) -> Result<Vec<Note>, LibraryError> {
        self.storage
            .notes(course_id, lesson_id)?
            .into_iter()
            .map(note)
            .collect()
    }

    fn check_anchor(
        &self,
        course_id: &str,
        lesson_id: &str,
        anchor: &Anchor,
    ) -> Result<(), LibraryError> {
        anchor.validate()?;
        let manifest = self
            .storage
            .course_manifest(course_id)?
            .ok_or(LibraryError::NoteTarget("the course is not in the library"))?;
        let expected = match anchor {
            Anchor::Lesson {} => None,
            Anchor::VideoTime { .. } => Some(ResourceKind::Video),
            Anchor::PdfPage { .. } => Some(ResourceKind::Pdf),
        };
        check_target(&manifest, lesson_id, anchor.resource_id(), expected)
    }
}

fn check_target(
    manifest: &CourseManifest,
    lesson_id: &str,
    resource_id: Option<&str>,
    expected: Option<ResourceKind>,
) -> Result<(), LibraryError> {
    let lesson = manifest
        .modules
        .iter()
        .flat_map(|module| &module.lessons)
        .find(|lesson| lesson.id == lesson_id)
        .ok_or(LibraryError::NoteTarget(
            "the lesson is not part of this course",
        ))?;
    let (Some(resource_id), Some(expected)) = (resource_id, expected) else {
        return Ok(());
    };
    let matches = lesson
        .resources
        .iter()
        .any(|resource| resource.id == resource_id && resource.kind == expected);
    if matches {
        Ok(())
    } else {
        Err(LibraryError::NoteTarget(
            "the note points at a resource this lesson does not have",
        ))
    }
}

fn note(record: NoteRecord) -> Result<Note, LibraryError> {
    let anchor = Anchor::from_json(&record.anchor_json)?;
    Ok(Note {
        note_id: record.note_id,
        lesson_id: record.lesson_id,
        anchor,
        body_json: record.body_json,
        text: record.body_text,
        revision: record.revision,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

fn change(write: NoteWrite) -> Result<NoteChange, LibraryError> {
    Ok(match write {
        NoteWrite::Saved(record) => NoteChange::Saved(note(record)?),
        NoteWrite::Conflict { current_revision } => NoteChange::Conflict { current_revision },
        NoteWrite::Missing => NoteChange::NotFound,
    })
}

fn note_id() -> Result<String, LibraryError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| LibraryError::NoteTarget("a note ID could not be generated"))?;
    let mut id = String::from("note_");
    for byte in bytes {
        let _ = write!(id, "{byte:02x}");
    }
    Ok(id)
}
