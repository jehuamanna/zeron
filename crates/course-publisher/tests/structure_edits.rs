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
    assert_eq!(course.modules[0].lessons.len(), 0);
    assert_eq!(course.remove_module(5, "module_a"), Ok(6));
    assert_eq!(course.modules.len(), 0);
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

fn resource(id: &str) -> course_publisher::DraftResource {
    course_publisher::DraftResource {
        id: id.to_owned(),
        kind: course_format::ResourceKind::Video,
        source_path: std::path::PathBuf::from("source.mp4"),
        package_path: format!("resources/{id}/source.mp4"),
        media_type: "video/mp4".to_owned(),
        duration_seconds: None,
        language: None,
    }
}

#[test]
fn resources_attach_reorder_and_detach_within_a_lesson() {
    let mut course = empty();
    course.add_module(0, module("module_a")).unwrap();
    course
        .add_lesson(1, "module_a", lesson("lesson_a"))
        .unwrap();
    assert_eq!(
        course.add_resource(2, "lesson_a", resource("resource_1")),
        Ok(3)
    );
    assert_eq!(
        course.add_resource(3, "lesson_a", resource("resource_2")),
        Ok(4)
    );
    assert_eq!(
        course.add_resource(4, "lesson_a", resource("module_a")),
        Err(EditError::DuplicateId {
            id: "module_a".to_owned()
        })
    );
    assert_eq!(course.move_resource(4, "lesson_a", "resource_2", 0), Ok(5));
    let order: Vec<_> = course.modules[0].lessons[0]
        .resources
        .iter()
        .map(|resource| resource.id.as_str())
        .collect();
    assert_eq!(order, ["resource_2", "resource_1"]);
    assert_eq!(course.remove_resource(5, "resource_2"), Ok(6));
    assert_eq!(course.modules[0].lessons[0].resources.len(), 1);
    assert_eq!(
        course.remove_resource(6, "resource_2"),
        Err(EditError::NotFound {
            entity: "resource",
            id: "resource_2".to_owned()
        })
    );
    assert_eq!(
        course.add_resource(6, "lesson_missing", resource("resource_3")),
        Err(EditError::NotFound {
            entity: "lesson",
            id: "lesson_missing".to_owned()
        })
    );
    assert_eq!(course.revision, 6);
}
