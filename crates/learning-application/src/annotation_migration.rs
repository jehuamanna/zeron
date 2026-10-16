//! Suggesting and applying new places for annotations after a course is
//! updated to a version that renamed, replaced, or removed lessons.

use std::collections::{BTreeMap, BTreeSet};

use course_format::CourseManifest;
use learning_domain::{
    Anchor, MatchReason, OutlineLesson, OutlineResource, remap_anchor, resource_map, suggest_lesson,
};
use learning_storage::AnnotationMoveRequest;

use crate::{CourseLibrary, LibraryError};

/// Annotations on a lesson the current course version no longer has, or
/// notes pointing at files a kept lesson no longer has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationMove {
    /// Lesson the annotations are on.
    pub from_lesson_id: String,
    /// Its title in the version it came from, when that version is known.
    pub from_title: Option<String>,
    /// That version.
    pub from_version: Option<String>,
    /// Suggested lesson of the current version, if one matches.
    pub suggested_lesson_id: Option<String>,
    /// Why it was suggested.
    pub reason: Option<MatchReason>,
    /// Notes on the lesson.
    pub notes: usize,
    /// Notes whose timestamp or page would point at the whole lesson after
    /// moving to the suggestion, because their file changed.
    pub notes_widened: usize,
    /// Whether the lesson has a drawing.
    pub has_drawing: bool,
    /// Whether the lesson has recorded progress.
    pub has_progress: bool,
}

/// What moving a lesson's annotations changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnotationMoveReport {
    /// Notes moved.
    pub notes_moved: usize,
    /// Of those, notes now pointing at the whole lesson.
    pub notes_widened: usize,
    /// The drawing moved.
    pub drawing_moved: bool,
    /// The target lesson already had a drawing, so this one stayed.
    pub drawing_kept: bool,
    /// Lesson progress moved.
    pub progress_moved: bool,
    /// Playback positions moved with their files.
    pub positions_moved: usize,
}

impl CourseLibrary {
    /// Every lesson whose annotations need a new place in the current
    /// version of a course, with a suggestion where one matches. Lessons are
    /// matched against the manifests of the versions the course replaced.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::NoteTarget`] for an unknown course or a
    /// storage failure.
    pub fn annotation_moves(&self, course_id: &str) -> Result<Vec<AnnotationMove>, LibraryError> {
        let current = self.current_manifest(course_id)?;
        let history = self.storage.previous_manifests(course_id)?;
        let current_lessons = outline(&current);
        let current_by_id: BTreeMap<&str, &OutlineLesson> = current_lessons
            .iter()
            .map(|lesson| (lesson.id.as_str(), lesson))
            .collect();
        let notes = self.storage.notes(course_id, None)?;
        let drawings: BTreeSet<String> = self
            .storage
            .drawing_lessons(course_id)?
            .into_iter()
            .collect();
        let progress: BTreeSet<String> = self
            .storage
            .lesson_progress(course_id)?
            .into_iter()
            .map(|record| record.lesson_id)
            .collect();
        let mut lessons: BTreeSet<&str> =
            notes.iter().map(|note| note.lesson_id.as_str()).collect();
        lessons.extend(drawings.iter().map(String::as_str));
        lessons.extend(progress.iter().map(String::as_str));

        let mut moves = Vec::new();
        for lesson_id in lessons {
            let anchors: Vec<Anchor> = notes
                .iter()
                .filter(|note| note.lesson_id == lesson_id)
                .filter_map(|note| Anchor::from_json(&note.anchor_json).ok())
                .collect();
            let previous = previous_lesson(&history, lesson_id);
            let (suggestion, reason) = if let Some(kept) = current_by_id.get(lesson_id) {
                let broken = anchors.iter().any(|anchor| {
                    anchor.resource_id().is_some_and(|resource| {
                        !kept.resources.iter().any(|other| other.id == resource)
                    })
                });
                if !broken {
                    continue;
                }
                (Some(lesson_id.to_owned()), Some(MatchReason::SameLesson))
            } else {
                previous
                    .as_ref()
                    .and_then(|(lesson, _)| suggest_lesson(lesson, &current_lessons))
                    .map_or((None, None), |(id, reason)| (Some(id), Some(reason)))
            };
            let source = source_lesson(&history, &current_lessons, lesson_id);
            let notes_widened = match (&suggestion, &source) {
                (Some(target), Some(lesson)) => {
                    let map = resource_map(lesson, current_by_id[target.as_str()]);
                    anchors
                        .iter()
                        .filter(|anchor| remap_anchor(anchor, &map).1)
                        .count()
                }
                _ => 0,
            };
            moves.push(AnnotationMove {
                from_lesson_id: lesson_id.to_owned(),
                from_title: previous.as_ref().map(|(lesson, _)| lesson.title.clone()),
                from_version: previous.map(|(_, version)| version),
                suggested_lesson_id: suggestion,
                reason,
                notes: anchors.len(),
                notes_widened,
                has_drawing: drawings.contains(lesson_id),
                has_progress: progress.contains(lesson_id),
            });
        }
        Ok(moves)
    }

    /// Move a lesson's annotations to a lesson of the current version, chosen
    /// by the learner or accepted from [`Self::annotation_moves`]. Note
    /// anchors follow files with identical contents; a note whose file
    /// changed points at the whole lesson. Nothing on the target is
    /// overwritten.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::NoteTarget`] for an unknown course or a target
    /// lesson the current version does not have, or a storage failure.
    pub fn move_annotations(
        &mut self,
        course_id: &str,
        from_lesson_id: &str,
        to_lesson_id: &str,
    ) -> Result<AnnotationMoveReport, LibraryError> {
        let current = self.current_manifest(course_id)?;
        let current_lessons = outline(&current);
        let target = current_lessons
            .iter()
            .find(|lesson| lesson.id == to_lesson_id)
            .ok_or(LibraryError::NoteTarget(
                "the course does not have that lesson",
            ))?;
        let history = self.storage.previous_manifests(course_id)?;
        let map = source_lesson(&history, &current_lessons, from_lesson_id)
            .map(|source| resource_map(&source, target))
            .unwrap_or_default();
        let mut notes_widened = 0;
        let note_anchors = self
            .storage
            .notes(course_id, Some(from_lesson_id))?
            .into_iter()
            .map(|note| {
                let anchor = Anchor::from_json(&note.anchor_json).unwrap_or(Anchor::Lesson {});
                let (anchor, widened) = remap_anchor(&anchor, &map);
                notes_widened += usize::from(widened);
                (note.note_id, anchor.to_json())
            })
            .collect();
        let outcome = self
            .storage
            .move_lesson_annotations(&AnnotationMoveRequest {
                course_id: course_id.to_owned(),
                from_lesson_id: from_lesson_id.to_owned(),
                to_lesson_id: to_lesson_id.to_owned(),
                note_anchors,
                resource_map: map.into_iter().collect(),
            })?;
        Ok(AnnotationMoveReport {
            notes_moved: outcome.notes_moved,
            notes_widened,
            drawing_moved: outcome.drawing_moved,
            drawing_kept: outcome.drawing_kept,
            progress_moved: outcome.progress_moved,
            positions_moved: outcome.positions_moved,
        })
    }

    fn current_manifest(&self, course_id: &str) -> Result<CourseManifest, LibraryError> {
        self.storage
            .course_manifest(course_id)?
            .ok_or(LibraryError::NoteTarget("the course is not in the library"))
    }
}

fn outline(manifest: &CourseManifest) -> Vec<OutlineLesson> {
    manifest
        .modules
        .iter()
        .flat_map(|module| &module.lessons)
        .map(|lesson| OutlineLesson {
            id: lesson.id.clone(),
            title: lesson.title.clone(),
            resources: lesson
                .resources
                .iter()
                .map(|resource| OutlineResource {
                    id: resource.id.clone(),
                    sha256: resource.sha256.clone(),
                })
                .collect(),
        })
        .collect()
}

/// The files a lesson's annotations may point at: those the current version
/// gives it, plus those of the newest replaced version that had it. Where
/// both name the same resource, the current one wins.
fn source_lesson(
    history: &[CourseManifest],
    current: &[OutlineLesson],
    lesson_id: &str,
) -> Option<OutlineLesson> {
    let kept = current
        .iter()
        .find(|lesson| lesson.id == lesson_id)
        .cloned();
    match (kept, previous_lesson(history, lesson_id)) {
        (Some(mut kept), Some((previous, _))) => {
            for resource in previous.resources {
                if !kept.resources.iter().any(|other| other.id == resource.id) {
                    kept.resources.push(resource);
                }
            }
            Some(kept)
        }
        (Some(kept), None) => Some(kept),
        (None, previous) => previous.map(|(lesson, _)| lesson),
    }
}

/// The newest replaced version that has this lesson, with that version.
fn previous_lesson(history: &[CourseManifest], lesson_id: &str) -> Option<(OutlineLesson, String)> {
    history.iter().find_map(|manifest| {
        outline(manifest)
            .into_iter()
            .find(|lesson| lesson.id == lesson_id)
            .map(|lesson| (lesson, manifest.version.clone()))
    })
}
