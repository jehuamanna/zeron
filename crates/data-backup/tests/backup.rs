//! Backups of live databases, verified staging, and crash-safe switching.

use std::{fs, path::PathBuf};

use data_backup::{
    BackupError, Database, apply_pending_restore, discard_pending_restore, pending_restore,
    stage_restore, write_backup,
};
use rusqlite::Connection;

const DATABASES: [Database; 2] = [
    Database {
        name: "library.sqlite3",
        schema_version: 14,
    },
    Database {
        name: "publisher.sqlite3",
        schema_version: 6,
    },
];

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("zeron-backup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A WAL-mode database with `rows` notes, kept open so its latest writes sit
/// in the journal.
fn live_database(dir: &Dir, name: &str, version: i64, rows: &[&str]) -> Connection {
    let connection = Connection::open(dir.0.join(name)).unwrap();
    connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TABLE notes (body TEXT NOT NULL); PRAGMA user_version = {version};"
        ))
        .unwrap();
    for row in rows {
        connection
            .execute("INSERT INTO notes (body) VALUES (?1)", [row])
            .unwrap();
    }
    connection
}

fn notes(path: &std::path::Path) -> Vec<String> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare("SELECT body FROM notes ORDER BY rowid")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn backup_of_live_data(name: &str) -> (Dir, Vec<u8>) {
    let source = Dir::new(name);
    let _library = live_database(&source, "library.sqlite3", 14, &["ownership", "borrowing"]);
    let _publisher = live_database(&source, "publisher.sqlite3", 6, &["draft"]);
    let mut bytes = Vec::new();
    let summary = write_backup(&source.0, &DATABASES, 1_760_000_000, &mut bytes).unwrap();
    assert_eq!(summary.entries.len(), 2);
    assert_eq!(summary.entries[0].schema_version, 14);
    assert!(!source.0.join(".backup-snapshots").exists());
    (source, bytes)
}

#[test]
fn a_backup_of_live_databases_restores_on_the_next_start() {
    let (_source, bytes) = backup_of_live_data("roundtrip");
    let target = Dir::new("roundtrip-target");
    drop(live_database(&target, "library.sqlite3", 14, &["old note"]));

    let summary = stage_restore(&target.0, &DATABASES, &mut bytes.as_slice()).unwrap();
    assert_eq!(summary.created_at, 1_760_000_000);
    assert_eq!(pending_restore(&target.0).unwrap().unwrap().len(), 2);
    assert_eq!(notes(&target.0.join("library.sqlite3")), ["old note"]);
    // A journal the previous run left beside the database must not be
    // replayed into the restored one.
    fs::write(target.0.join("library.sqlite3-wal"), b"stale journal").unwrap();

    let aside = apply_pending_restore(&target.0, 42).unwrap().unwrap();
    assert!(aside.ends_with("before-restore-42"));
    assert_eq!(
        notes(&target.0.join("library.sqlite3")),
        ["ownership", "borrowing"]
    );
    assert_eq!(notes(&target.0.join("publisher.sqlite3")), ["draft"]);
    assert_eq!(
        fs::read(aside.join("library.sqlite3-wal")).unwrap(),
        b"stale journal"
    );
    assert_eq!(notes(&aside.join("library.sqlite3")), ["old note"]);
    assert!(!target.0.join("library.sqlite3-wal").exists());
    assert_eq!(pending_restore(&target.0).unwrap(), None);
    assert_eq!(apply_pending_restore(&target.0, 43).unwrap(), None);
}

#[test]
fn an_interrupted_switch_finishes_on_the_next_start() {
    let (_source, bytes) = backup_of_live_data("resume");
    let target = Dir::new("resume-target");
    drop(live_database(&target, "publisher.sqlite3", 6, &["old"]));
    stage_restore(&target.0, &DATABASES, &mut bytes.as_slice()).unwrap();
    // As if the first start crashed after switching in the library only.
    fs::rename(
        target.0.join(".restore-staging/library.sqlite3"),
        target.0.join("library.sqlite3"),
    )
    .unwrap();
    apply_pending_restore(&target.0, 7).unwrap().unwrap();
    assert_eq!(
        notes(&target.0.join("library.sqlite3")),
        ["ownership", "borrowing"]
    );
    assert_eq!(notes(&target.0.join("publisher.sqlite3")), ["draft"]);
    assert_eq!(pending_restore(&target.0).unwrap(), None);
}

#[test]
fn changed_foreign_and_newer_backups_are_refused_and_nothing_is_staged() {
    let (_source, bytes) = backup_of_live_data("refuse");
    let target = Dir::new("refuse-target");
    let refused = |input: &[u8], databases: &[Database]| {
        let result = stage_restore(&target.0, databases, &mut &input[..]);
        assert_eq!(pending_restore(&target.0).unwrap(), None);
        assert!(!target.0.join(".restore-staging").exists());
        result.unwrap_err()
    };
    assert!(matches!(
        refused(b"PK\x03\x04 zip", &DATABASES),
        BackupError::Invalid(_)
    ));
    assert!(matches!(
        refused(&bytes[..bytes.len() - 10], &DATABASES),
        BackupError::Invalid(_)
    ));
    let mut flipped = bytes.clone();
    let last = flipped.len() - 100;
    flipped[last] ^= 0xff;
    assert!(matches!(
        refused(&flipped, &DATABASES),
        BackupError::Invalid(_)
    ));
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(matches!(
        refused(&trailing, &DATABASES),
        BackupError::Invalid(_)
    ));
    let older_build = [
        Database {
            name: "library.sqlite3",
            schema_version: 13,
        },
        DATABASES[1],
    ];
    assert!(matches!(refused(&bytes, &older_build), BackupError::TooNew));
    assert!(matches!(
        refused(&bytes, &DATABASES[..1]),
        BackupError::Invalid(_)
    ));

    stage_restore(&target.0, &DATABASES, &mut bytes.as_slice()).unwrap();
    discard_pending_restore(&target.0).unwrap();
    assert_eq!(pending_restore(&target.0).unwrap(), None);
    assert_eq!(apply_pending_restore(&target.0, 1).unwrap(), None);
}
