//! Revision-safe module and lesson structure edit tests.

use course_publisher::{DraftCourse, DraftLesson, DraftMetadata, DraftModule, EditError};

fn empty() -> DraftCourse {
    DraftCourse {
        draft_id: "draft_one".to_owned(),
        revision: 0,
        metadata: DraftMetadata {
            course_id: "course_one".to_owned(),
            version: "1.0.0".to_owned(),
            title: "Course".to_owned(),
            description: None,
            publisher_name: "Publisher".to_owned(),
            language: "en".to_owned(),
            category: None,
            tags: vec![],
            prerequisites: vec![],
            license_name: None,
        },
        modules: vec![],
    }
}

fn module(id: &str) -> DraftModule {
    DraftModule {
        id: id.to_owned(),
        title: id.to_owned(),
        lessons: vec![],
    }
}

fn lesson(id: &str) -> DraftLesson {
    DraftLesson {
        id: id.to_owned(),
        title: id.to_owned(),
        estimated_duration_seconds: None,
        resources: vec![],
    }
}

#[test]
fn add_rename_and_remove_advance_one_revision_each() {
    let mut course = empty();
    assert_eq!(course.add_module(0, module("module_a")), Ok(1));
    assert_eq!(course.add_lesson(1, "module_a", lesson("lesson_a")), Ok(2));
    assert_eq!(
        course.rename_module(2, "module_a", "Basics".to_owned()),
        Ok(3)
    );
    assert_eq!(
        course.rename_lesson(3, "lesson_a", "Hello".to_owned()),
        Ok(4)
    );
    assert_eq!(course.modules[0].title, "Basics");
    assert_eq!(course.modules[0].lessons[0].title, "Hello");
    assert_eq!(course.remove_lesson(4, "lesson_a"), Ok(5));
    assert!(course.modules[0].lessons.is_empty());
    assert_eq!(course.remove_module(5, "module_a"), Ok(6));
    assert!(course.modules.is_empty());
}

#[test]
fn stale_structure_edit_changes_nothing() {
    let mut course = empty();
    course.add_module(0, module("module_a")).unwrap();
    let before = course.clone();
    assert_eq!(
        course.add_lesson(0, "module_a", lesson("lesson_a")),
        Err(EditError::RevisionConflict {
            expected: 0,
            actual: 1
        })
    );
    assert_eq!(
        course.remove_module(0, "module_a"),
        Err(EditError::RevisionConflict {
            expected: 0,
            actual: 1
        })
    );
    assert_eq!(course, before);
}

#[test]
fn ids_are_unique_across_modules_and_lessons() {
    let mut course = empty();
    course.add_module(0, module("shared")).unwrap();
    assert_eq!(
        course.add_module(1, module("shared")),
        Err(EditError::DuplicateId {
            id: "shared".to_owned()
        })
    );
    assert_eq!(
        course.add_lesson(1, "shared", lesson("shared")),
        Err(EditError::DuplicateId {
            id: "shared".to_owned()
        })
    );
    assert_eq!(course.revision, 1);
}

#[test]
fn missing_targets_are_reported_by_entity() {
    let mut course = empty();
    assert_eq!(
        course.add_lesson(0, "module_missing", lesson("lesson_a")),
        Err(EditError::NotFound {
            entity: "module",
            id: "module_missing".to_owned()
        })
    );
    assert_eq!(
        course.rename_lesson(0, "lesson_missing", "x".to_owned()),
        Err(EditError::NotFound {
            entity: "lesson",
            id: "lesson_missing".to_owned()
        })
    );
    assert_eq!(
        course.remove_lesson(0, "lesson_missing"),
        Err(EditError::NotFound {
            entity: "lesson",
            id: "lesson_missing".to_owned()
        })
    );
    assert_eq!(course.revision, 0);
}

#[test]
fn lessons_reorder_within_their_module() {
    let mut course = empty();
    course.add_module(0, module("module_a")).unwrap();
    for (revision, id) in [(1, "lesson_1"), (2, "lesson_2"), (3, "lesson_3")] {
        course.add_lesson(revision, "module_a", lesson(id)).unwrap();
    }
    assert_eq!(course.move_lesson(4, "module_a", "lesson_3", 0), Ok(5));
    let order: Vec<_> = course.modules[0]
        .lessons
        .iter()
        .map(|lesson| lesson.id.as_str())
        .collect();
    assert_eq!(order, ["lesson_3", "lesson_1", "lesson_2"]);
    assert_eq!(
        course.move_lesson(5, "module_a", "lesson_1", 3),
        Err(EditError::InvalidPosition {
            entity: "lesson",
            position: 3
        })
    );
}
