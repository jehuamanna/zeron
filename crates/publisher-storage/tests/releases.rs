//! Immutable release record tests.

use std::fs;

use course_publisher::{DraftCourse, DraftMetadata};
use publisher_storage::{NewRelease, PublisherStorage, PublisherStorageError, ReleaseStatus};

fn release(release_id: &str, version: &str) -> NewRelease {
    NewRelease {
        release_id: release_id.to_owned(),
        course_id: "course_one".to_owned(),
        course_version: version.to_owned(),
        title: "Course".to_owned(),
        draft_id: "draft_one".to_owned(),
        draft_revision: 9,
        manifest_sha256: "f".repeat(64),
        info_hash: "a".repeat(40),
        magnet: "magnet:?xt=urn:btih:aaaa".to_owned(),
        piece_length: 16_384,
        piece_count: 3,
        file_count: 2,
        total_bytes: 40_000,
    }
}

fn draft() -> DraftCourse {
    DraftCourse {
        draft_id: "draft_one".to_owned(),
        revision: 0,
        metadata: DraftMetadata {
            course_id: "course_one".to_owned(),
            version: "1.2.0".to_owned(),
            title: "Course".to_owned(),
            description: None,
            publisher_name: "P".to_owned(),
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
fn a_new_release_supersedes_the_previous_published_release() {
    let mut storage = PublisherStorage::open_in_memory().unwrap();
    let first = storage
        .insert_release(release("release_1", "1.0.0"))
        .unwrap();
    assert_eq!(first.status, ReleaseStatus::Published);
    assert!(storage.version_released("course_one", "1.0.0").unwrap());
    assert!(!storage.version_released("course_one", "1.1.0").unwrap());

    storage
        .insert_release(release("release_2", "1.1.0"))
        .unwrap();
    let statuses: Vec<_> = storage
        .releases()
        .unwrap()
        .into_iter()
        .map(|record| (record.release.release_id, record.status))
        .collect();
    assert!(statuses.contains(&("release_1".to_owned(), ReleaseStatus::Superseded)));
    assert!(statuses.contains(&("release_2".to_owned(), ReleaseStatus::Published)));
    let loaded = storage.release("release_2").unwrap().unwrap();
    assert_eq!(loaded.release, release("release_2", "1.1.0"));
    assert!(storage.release("release_missing").unwrap().is_none());
}

#[test]
fn a_released_version_can_never_be_recorded_again() {
    let mut storage = PublisherStorage::open_in_memory().unwrap();
    storage
        .insert_release(release("release_1", "1.0.0"))
        .unwrap();
    assert!(matches!(
        storage.insert_release(release("release_2", "1.0.0")),
        Err(PublisherStorageError::VersionAlreadyReleased { course_version }) if course_version == "1.0.0"
    ));
    let records = storage.releases().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, ReleaseStatus::Published);
}

#[test]
fn version_one_databases_migrate_without_losing_drafts() {
    let database = std::env::temp_dir().join(format!(
        "zeron-publisher-migrate-{}.sqlite3",
        std::process::id()
    ));
    let _ = fs::remove_file(&database);
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE drafts (
                draft_id TEXT PRIMARY KEY NOT NULL,
                course_id TEXT NOT NULL,
                course_version TEXT NOT NULL,
                title TEXT NOT NULL,
                revision INTEGER NOT NULL CHECK (revision >= 0),
                snapshot_json BLOB NOT NULL,
                updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
             );
             PRAGMA user_version = 1;",
        )
        .unwrap();
    let draft = DraftCourse {
        draft_id: "draft_old".to_owned(),
        revision: 0,
        metadata: DraftMetadata {
            course_id: "course_old".to_owned(),
            version: "0.1.0".to_owned(),
            title: "Old".to_owned(),
            description: None,
            publisher_name: "P".to_owned(),
            language: "en".to_owned(),
            category: None,
            tags: vec![],
            prerequisites: vec![],
            license_name: None,
        },
        modules: vec![],
    };
    connection
        .execute(
            "INSERT INTO drafts VALUES ('draft_old', 'course_old', '0.1.0', 'Old', 0, ?1, 5)",
            [serde_json::to_vec(&draft).unwrap()],
        )
        .unwrap();
    drop(connection);

    let mut storage = PublisherStorage::open(&database).unwrap();
    assert_eq!(storage.draft("draft_old").unwrap().unwrap(), draft);
    storage
        .insert_release(release("release_1", "1.0.0"))
        .unwrap();
    drop(storage);
    let version: i64 = rusqlite::Connection::open(&database)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 3);
    let reopened = PublisherStorage::open(&database).unwrap();
    assert_eq!(reopened.releases().unwrap().len(), 1);
    fs::remove_file(database).unwrap();
}

#[test]
fn seeding_intents_and_settings_persist() {
    let mut storage = PublisherStorage::open_in_memory().unwrap();
    storage
        .insert_release(release("release_1", "1.0.0"))
        .unwrap();
    storage
        .insert_release(release("release_2", "1.1.0"))
        .unwrap();
    assert_eq!(storage.seeding_intents().unwrap().len(), 0);
    storage.set_seeding_intent("release_1", true).unwrap();
    storage.set_seeding_intent("release_1", true).unwrap();
    storage.set_seeding_intent("release_2", true).unwrap();
    let mut intents = storage.seeding_intents().unwrap();
    intents.sort();
    assert_eq!(intents, ["release_1", "release_2"]);
    storage.set_seeding_intent("release_1", false).unwrap();
    assert_eq!(storage.seeding_intents().unwrap(), ["release_2"]);
    assert!(storage.set_seeding_intent("release_missing", true).is_err());

    assert_eq!(storage.setting("network_profile").unwrap(), None);
    storage.set_setting("network_profile", "internet").unwrap();
    storage
        .set_setting("network_profile", "local_network")
        .unwrap();
    assert_eq!(
        storage.setting("network_profile").unwrap().as_deref(),
        Some("local_network")
    );
}

#[test]
fn archiving_drops_the_seeding_intent_and_keeps_other_releases() {
    let mut storage = PublisherStorage::open_in_memory().unwrap();
    storage.create_draft(&draft()).unwrap();
    storage
        .insert_release(release("release_1", "1.0.0"))
        .unwrap();
    storage
        .insert_release(release("release_2", "1.1.0"))
        .unwrap();
    storage.set_seeding_intent("release_1", true).unwrap();
    storage.set_seeding_intent("release_2", true).unwrap();

    let archived = storage.archive_release("release_2").unwrap().unwrap();
    assert_eq!(archived.status, ReleaseStatus::Archived);
    assert_eq!(archived.release, release("release_2", "1.1.0"));
    assert_eq!(storage.seeding_intents().unwrap(), ["release_1"]);
    assert_eq!(
        storage.release("release_1").unwrap().unwrap().status,
        ReleaseStatus::Superseded
    );
    assert_eq!(
        storage
            .archive_release("release_2")
            .unwrap()
            .unwrap()
            .status,
        ReleaseStatus::Archived
    );
    assert!(
        storage
            .archive_release("release_missing")
            .unwrap()
            .is_none()
    );
    assert!(storage.version_released("course_one", "1.1.0").unwrap());
    assert!(matches!(
        storage.insert_release(release("release_3", "1.1.0")),
        Err(PublisherStorageError::VersionAlreadyReleased { .. })
    ));

    assert!(storage.delete_draft("draft_one", 0).unwrap());
    assert_eq!(storage.releases().unwrap().len(), 2);
}
