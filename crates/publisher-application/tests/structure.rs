//! Structure editing use-case tests.

use publisher_application::{
    DraftServiceError, MetadataEdit, PublisherDrafts, StoredDraft, StructureEdit,
};

fn add_module(drafts: &mut PublisherDrafts, id: &str, revision: u64, title: &str) -> StoredDraft {
    drafts
        .edit_structure(
            id,
            revision,
            StructureEdit::AddModule {
                title: title.to_owned(),
            },
        )
        .unwrap()
}

#[test]
fn builds_and_reorders_a_course_outline() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let id = drafts.create().unwrap().draft.draft_id;
    let first = add_module(&mut drafts, &id, 0, "Basics");
    let module_a = first.draft.modules[0].id.clone();
    assert!(module_a.starts_with("module_"));
    let second = add_module(&mut drafts, &id, 1, "Advanced");
    let module_b = second.draft.modules[1].id.clone();
    let moved = drafts
        .edit_structure(
            &id,
            2,
            StructureEdit::MoveModule {
                module_id: module_b.clone(),
                position: 0,
            },
        )
        .unwrap();
    assert_eq!(moved.draft.modules[0].id, module_b);
    let with_lesson = drafts
        .edit_structure(
            &id,
            3,
            StructureEdit::AddLesson {
                module_id: module_a.clone(),
                title: "Hello".to_owned(),
            },
        )
        .unwrap();
    let lesson = &with_lesson.draft.modules[1].lessons[0];
    assert!(lesson.id.starts_with("lesson_"));
    assert_eq!(lesson.title, "Hello");
    let renamed = drafts
        .edit_structure(
            &id,
            4,
            StructureEdit::RenameLesson {
                lesson_id: lesson.id.clone(),
                title: "Hello, world".to_owned(),
            },
        )
        .unwrap();
    assert_eq!(renamed.draft.revision, 5);
    let durable = drafts.draft(&id).unwrap().unwrap();
    assert_eq!(durable.draft, renamed.draft);
    assert_eq!(durable.draft.modules[1].lessons[0].title, "Hello, world");
}

#[test]
fn removal_drops_nested_lessons() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let id = drafts.create().unwrap().draft.draft_id;
    let module_id = add_module(&mut drafts, &id, 0, "Basics").draft.modules[0]
        .id
        .clone();
    drafts
        .edit_structure(
            &id,
            1,
            StructureEdit::AddLesson {
                module_id: module_id.clone(),
                title: "Lesson".to_owned(),
            },
        )
        .unwrap();
    let removed = drafts
        .edit_structure(&id, 2, StructureEdit::RemoveModule { module_id })
        .unwrap();
    assert!(removed.draft.modules.is_empty());
    assert_eq!(removed.draft.revision, 3);
}

#[test]
fn structure_and_metadata_share_one_revision_sequence() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let id = drafts.create().unwrap().draft.draft_id;
    add_module(&mut drafts, &id, 0, "Basics");
    let stale_metadata = drafts.update_metadata(
        &id,
        0,
        MetadataEdit {
            version: "1.0.0".to_owned(),
            title: "Stale".to_owned(),
            publisher_name: "P".to_owned(),
            language: "en".to_owned(),
            ..MetadataEdit::default()
        },
    );
    assert!(matches!(
        stale_metadata,
        Err(DraftServiceError::RevisionConflict {
            expected: 0,
            actual: 1
        })
    ));
    let stale_structure = drafts.edit_structure(
        &id,
        0,
        StructureEdit::AddModule {
            title: "Late".to_owned(),
        },
    );
    assert!(matches!(
        stale_structure,
        Err(DraftServiceError::RevisionConflict {
            expected: 0,
            actual: 1
        })
    ));
    assert_eq!(drafts.draft(&id).unwrap().unwrap().draft.modules.len(), 1);
}

#[test]
fn rejects_missing_targets_bad_positions_and_unbounded_titles() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let id = drafts.create().unwrap().draft.draft_id;
    let module_id = add_module(&mut drafts, &id, 0, "Basics").draft.modules[0]
        .id
        .clone();
    assert!(matches!(
        drafts.edit_structure(
            &id,
            1,
            StructureEdit::RemoveLesson {
                lesson_id: "lesson_missing".to_owned()
            }
        ),
        Err(DraftServiceError::EntityNotFound {
            entity: "lesson",
            ..
        })
    ));
    assert!(matches!(
        drafts.edit_structure(
            &id,
            1,
            StructureEdit::MoveModule {
                module_id: module_id.clone(),
                position: 1
            }
        ),
        Err(DraftServiceError::InvalidPosition {
            entity: "module",
            position: 1
        })
    ));
    match drafts.edit_structure(
        &id,
        1,
        StructureEdit::RenameModule {
            module_id,
            title: "a\nb".to_owned(),
        },
    ) {
        Err(DraftServiceError::InvalidInput { field, .. }) => assert_eq!(field, "module.title"),
        other => panic!("expected invalid title, got {other:?}"),
    }
    assert!(matches!(
        drafts.edit_structure(
            &id,
            1,
            StructureEdit::AddModule {
                title: "x".repeat(201)
            }
        ),
        Err(DraftServiceError::InvalidInput { .. })
    ));
    assert_eq!(drafts.draft(&id).unwrap().unwrap().draft.revision, 1);
}
