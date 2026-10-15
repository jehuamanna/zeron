//! Durable publisher draft tests.

use std::fs;

use course_publisher::{DraftCourse, DraftMetadata};
use publisher_storage::{PublisherStorage, PublisherStorageError};

fn draft() -> DraftCourse {
    DraftCourse {
        draft_id: "draft_one".to_owned(),
        revision: 0,
        metadata: DraftMetadata {
            course_id: "course_one".to_owned(),
            version: "1.0.0".to_owned(),
            title: "Original title".to_owned(),
            description: None,
            publisher_name: "Zeron Tests".to_owned(),
            language: "en".to_owned(),
            category: None,
            tags: vec![],
            prerequisites: vec![],
            license_name: None,
        },
        modules: vec![],
    }
}

#[test]
fn draft_survives_restart_and_lists_without_source_paths() {
    let database = std::env::temp_dir().join(format!(
        "zeron-publisher-storage-{}.sqlite3",
        std::process::id()
    ));
    let _ = fs::remove_file(&database);
    {
        let mut storage = PublisherStorage::open(&database).unwrap();
        storage.create_draft(&draft()).unwrap();
    }
    let storage = PublisherStorage::open(&database).unwrap();
    let recovered = storage.draft("draft_one").unwrap().unwrap();
    assert_eq!(recovered, draft());
    let summaries = storage.list_drafts().unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].title, "Original title");
    fs::remove_file(database).unwrap();
}

#[test]
fn stored_draft_reports_the_last_save_time() {
    let mut storage = PublisherStorage::open_in_memory().unwrap();
    let summary = storage.create_draft(&draft()).unwrap();
    let stored = storage.stored_draft("draft_one").unwrap().unwrap();
    assert_eq!(stored.draft, draft());
    assert_eq!(
        stored.updated_at_unix_seconds,
        summary.updated_at_unix_seconds
    );
    assert!(storage.stored_draft("draft_missing").unwrap().is_none());
}

#[test]
fn compare_and_swap_rejects_a_stale_autosave() {
    let mut storage = PublisherStorage::open_in_memory().unwrap();
    let original = draft();
    storage.create_draft(&original).unwrap();
    let mut first_editor = original.clone();
    first_editor.revision = 1;
    first_editor.metadata.title = "First editor".to_owned();
    storage.update_draft(0, &first_editor).unwrap();

    let mut stale_editor = original;
    stale_editor.revision = 1;
    stale_editor.metadata.title = "Stale editor".to_owned();
    assert!(matches!(
        storage.update_draft(0, &stale_editor),
        Err(PublisherStorageError::RevisionConflict {
            expected: 0,
            actual: 1
        })
    ));
    assert_eq!(
        storage.draft("draft_one").unwrap().unwrap().metadata.title,
        "First editor"
    );
}

#[test]
fn update_requires_exactly_one_revision_step() {
    let mut storage = PublisherStorage::open_in_memory().unwrap();
    let mut snapshot = draft();
    storage.create_draft(&snapshot).unwrap();
    snapshot.revision = 2;
    assert!(matches!(
        storage.update_draft(0, &snapshot),
        Err(PublisherStorageError::InvalidRevisionAdvance {
            expected: 1,
            actual: 2
        })
    ));
}

#[test]
fn rejects_a_future_database_schema() {
    let database = std::env::temp_dir().join(format!(
        "zeron-publisher-future-{}.sqlite3",
        std::process::id()
    ));
    let _ = fs::remove_file(&database);
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);
    assert!(matches!(
        PublisherStorage::open(&database),
        Err(PublisherStorageError::FutureSchema {
            found: 99,
            supported: 3
        })
    ));
    fs::remove_file(database).unwrap();
}
