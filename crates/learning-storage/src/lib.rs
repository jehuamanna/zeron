//! `SQLite` persistence adapters for Zeron.
//!
//! Database rows and `rusqlite` types remain private to this crate. The API is
//! synchronous and intended to run on a dedicated database thread owned by a
//! composition root.

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use course_format::{CourseManifest, ManifestError, parse_and_validate};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use thiserror::Error;

const CURRENT_SCHEMA_VERSION: i64 = 2;

/// Durable course metadata returned to application use cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCourseSummary {
    /// Stable logical course ID.
    pub course_id: String,
    /// Imported course version.
    pub course_version: String,
    /// Display title.
    pub title: String,
    /// Publisher display name.
    pub publisher_name: String,
    /// Language tag.
    pub language: String,
    /// Sum of declared and verified resource bytes.
    pub total_resource_bytes: u64,
    /// Unix timestamp in seconds at successful import.
    pub imported_at_unix_seconds: u64,
}

/// Storage startup and operation failures.
#[derive(Debug, Error)]
pub enum StorageError {
    /// `SQLite` rejected an operation.
    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A manifest snapshot could not be serialized or parsed.
    #[error("course snapshot JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    /// A stored manifest no longer satisfies the semantic contract.
    #[error("stored course snapshot is invalid: {0}")]
    Manifest(#[from] ManifestError),
    /// This `SQLite` build lacks a mandatory capability.
    #[error("SQLite capability unavailable: {0}")]
    MissingCapability(&'static str),
    /// An integer cannot be represented safely by `SQLite`.
    #[error("value is too large for SQLite: {0}")]
    IntegerOverflow(&'static str),
    /// The stored schema is newer than this application understands.
    #[error("database schema version {found} is newer than supported version {supported}")]
    FutureSchema {
        /// Version found in the database.
        found: i64,
        /// Maximum understood version.
        supported: i64,
    },
    /// The system clock cannot produce a Unix timestamp.
    #[error("system clock is before the Unix epoch")]
    InvalidSystemClock,
}

/// An opened and migrated `SQLite` database.
pub struct Storage {
    connection: Connection,
}

impl Storage {
    /// Open a database file, negotiate safe pragmas, run migrations, and prove
    /// mandatory `SQLite` capabilities before returning readiness.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if opening, configuration, migration, or the
    /// FTS5 capability smoke test fails.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let connection = Connection::open(path)?;
        Self::initialize(connection)
    }

    /// Open an isolated in-memory database with the production migrations.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when configuration, migration, or capability
    /// verification fails.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(mut connection: Connection) -> Result<Self, StorageError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let journal_mode: String =
            connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        if journal_mode != "wal" && journal_mode != "memory" {
            return Err(StorageError::MissingCapability("WAL journal mode"));
        }
        if !connection.is_autocommit() {
            return Err(StorageError::Sqlite(rusqlite::Error::InvalidQuery));
        }
        migrate(&mut connection)?;
        verify_capabilities(&connection)?;
        Ok(Self { connection })
    }

    /// Persist a fully verified manifest snapshot transactionally.
    ///
    /// Reimporting the same logical course replaces its current library
    /// snapshot and resource rows atomically. Learner-owned data will live in
    /// separate tables and is not touched by this operation.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when serialization, integer conversion, clock,
    /// or `SQLite` operations fail.
    pub fn save_imported_course(
        &mut self,
        manifest: &CourseManifest,
        total_resource_bytes: u64,
        package_root: &Path,
    ) -> Result<StoredCourseSummary, StorageError> {
        let imported_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StorageError::InvalidSystemClock)?
            .as_secs();
        self.save_imported_course_at(manifest, total_resource_bytes, package_root, imported_at)
    }

    /// Fetch one course summary by stable logical ID.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for `SQLite` failures or corrupt negative numeric
    /// values in the database.
    pub fn course(&self, course_id: &str) -> Result<Option<StoredCourseSummary>, StorageError> {
        self.connection
            .query_row(
                "SELECT course_id, course_version, title, publisher_name, language, total_resource_bytes, imported_at FROM courses WHERE course_id = ?1",
                [course_id],
                summary_from_row,
            )
            .optional()
            .map_err(StorageError::from)
    }

    /// List course summaries in deterministic title/ID order.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for `SQLite` query or row-decoding failures.
    pub fn list_courses(&self) -> Result<Vec<StoredCourseSummary>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT course_id, course_version, title, publisher_name, language, total_resource_bytes, imported_at FROM courses ORDER BY title COLLATE NOCASE, course_id",
        )?;
        let rows = statement.query_map([], summary_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    /// Load and parse the exact manifest snapshot stored for a course.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for `SQLite` or JSON failures.
    pub fn course_manifest(&self, course_id: &str) -> Result<Option<CourseManifest>, StorageError> {
        let json: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT manifest_json FROM courses WHERE course_id = ?1",
                [course_id],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|bytes| parse_and_validate(&bytes))
            .transpose()
            .map_err(StorageError::from)
    }

    /// Resolve the backend-only package root for a local course.
    ///
    /// This value must not cross the IPC boundary.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures or a path encoding that is
    /// incompatible with the current operating system.
    pub fn course_package_root(&self, course_id: &str) -> Result<Option<PathBuf>, StorageError> {
        let encoded: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT path_encoding, path_bytes FROM course_locations WHERE course_id = ?1",
                [course_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        encoded
            .map(|(encoding, bytes)| decode_path(&encoding, &bytes))
            .transpose()
    }

    /// Search imported course metadata through `SQLite` FTS5.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when `SQLite` rejects the bound FTS query. The
    /// caller should present syntax errors as recoverable user input errors.
    pub fn search_courses(&self, query: &str) -> Result<Vec<StoredCourseSummary>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT c.course_id, c.course_version, c.title, c.publisher_name, c.language, c.total_resource_bytes, c.imported_at
             FROM course_search AS s JOIN courses AS c ON c.course_id = s.course_id
             WHERE course_search MATCH ?1
             ORDER BY bm25(course_search), c.course_id",
        )?;
        let rows = statement.query_map([query], summary_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    fn save_imported_course_at(
        &mut self,
        manifest: &CourseManifest,
        total_resource_bytes: u64,
        package_root: &Path,
        imported_at: u64,
    ) -> Result<StoredCourseSummary, StorageError> {
        let total_bytes = sqlite_integer(total_resource_bytes, "total resource bytes")?;
        let imported_at_i64 = sqlite_integer(imported_at, "import timestamp")?;
        let manifest_json = serde_json::to_vec(manifest)?;
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO courses (course_id, course_version, title, description, publisher_name, language, manifest_json, total_resource_bytes, imported_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(course_id) DO UPDATE SET
               course_version=excluded.course_version,
               title=excluded.title,
               description=excluded.description,
               publisher_name=excluded.publisher_name,
               language=excluded.language,
               manifest_json=excluded.manifest_json,
               total_resource_bytes=excluded.total_resource_bytes,
               imported_at=excluded.imported_at",
            params![
                manifest.course_id,
                manifest.version,
                manifest.title,
                manifest.description,
                manifest.publisher.name,
                manifest.language,
                manifest_json,
                total_bytes,
                imported_at_i64,
            ],
        )?;
        tx.execute(
            "DELETE FROM course_resources WHERE course_id = ?1",
            [&manifest.course_id],
        )?;
        for resource in manifest
            .modules
            .iter()
            .flat_map(|module| &module.lessons)
            .flat_map(|lesson| &lesson.resources)
        {
            tx.execute(
                "INSERT INTO course_resources (course_id, resource_id, package_path, media_type, byte_length, sha256)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    manifest.course_id,
                    resource.id,
                    resource.path,
                    resource.media_type,
                    sqlite_integer(resource.byte_length, "resource byte length")?,
                    resource.sha256,
                ],
            )?;
        }
        let (path_encoding, path_bytes) = encode_path(package_root);
        tx.execute(
            "INSERT INTO course_locations (course_id, path_encoding, path_bytes, verified_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(course_id) DO UPDATE SET
               path_encoding=excluded.path_encoding,
               path_bytes=excluded.path_bytes,
               verified_at=excluded.verified_at",
            params![
                manifest.course_id,
                path_encoding,
                path_bytes,
                imported_at_i64
            ],
        )?;
        refresh_search_row(&tx, manifest)?;
        tx.commit()?;

        Ok(StoredCourseSummary {
            course_id: manifest.course_id.clone(),
            course_version: manifest.version.clone(),
            title: manifest.title.clone(),
            publisher_name: manifest.publisher.name.clone(),
            language: manifest.language.clone(),
            total_resource_bytes,
            imported_at_unix_seconds: imported_at,
        })
    }
}

fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    let mut found: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if found > CURRENT_SCHEMA_VERSION {
        return Err(StorageError::FutureSchema {
            found,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    if found == 0 {
        let tx = connection.transaction()?;
        tx.execute_batch(
            "CREATE TABLE courses (
               course_id TEXT PRIMARY KEY NOT NULL,
               course_version TEXT NOT NULL,
               title TEXT NOT NULL,
               description TEXT,
               publisher_name TEXT NOT NULL,
               language TEXT NOT NULL,
               manifest_json BLOB NOT NULL,
               total_resource_bytes INTEGER NOT NULL CHECK(total_resource_bytes >= 0),
               imported_at INTEGER NOT NULL CHECK(imported_at >= 0)
             ) STRICT;
             CREATE TABLE course_resources (
               course_id TEXT NOT NULL REFERENCES courses(course_id) ON DELETE CASCADE,
               resource_id TEXT NOT NULL,
               package_path TEXT NOT NULL,
               media_type TEXT NOT NULL,
               byte_length INTEGER NOT NULL CHECK(byte_length >= 0),
               sha256 TEXT NOT NULL,
               PRIMARY KEY(course_id, resource_id),
               UNIQUE(course_id, package_path)
             ) STRICT;
             CREATE VIRTUAL TABLE course_search USING fts5(
               course_id UNINDEXED,
               title,
               description,
               publisher_name,
               tags,
               tokenize = 'unicode61'
             );
             PRAGMA user_version = 1;",
        )?;
        tx.commit()?;
        found = 1;
    }
    if found == 1 {
        let tx = connection.transaction()?;
        tx.execute_batch(
            "CREATE TABLE course_locations (
               course_id TEXT PRIMARY KEY NOT NULL REFERENCES courses(course_id) ON DELETE CASCADE,
               path_encoding TEXT NOT NULL,
               path_bytes BLOB NOT NULL,
               verified_at INTEGER NOT NULL CHECK(verified_at >= 0)
             ) STRICT;
             PRAGMA user_version = 2;",
        )?;
        tx.commit()?;
    }
    Ok(())
}

fn verify_capabilities(connection: &Connection) -> Result<(), StorageError> {
    let enabled: i64 = connection.query_row(
        "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
        [],
        |row| row.get(0),
    )?;
    if enabled != 1 {
        return Err(StorageError::MissingCapability("FTS5"));
    }
    connection.query_row("SELECT count(*) FROM course_search", [], |_| Ok(()))?;
    let foreign_keys: i64 =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    if foreign_keys != 1 {
        return Err(StorageError::MissingCapability("foreign key enforcement"));
    }
    Ok(())
}

fn refresh_search_row(tx: &Transaction<'_>, manifest: &CourseManifest) -> Result<(), StorageError> {
    tx.execute(
        "DELETE FROM course_search WHERE course_id = ?1",
        [&manifest.course_id],
    )?;
    tx.execute(
        "INSERT INTO course_search (course_id, title, description, publisher_name, tags) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            manifest.course_id,
            manifest.title,
            manifest.description.as_deref().unwrap_or_default(),
            manifest.publisher.name,
            manifest.tags.join(" "),
        ],
    )?;
    Ok(())
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredCourseSummary> {
    let total_resource_bytes: i64 = row.get(5)?;
    let imported_at: i64 = row.get(6)?;
    Ok(StoredCourseSummary {
        course_id: row.get(0)?,
        course_version: row.get(1)?,
        title: row.get(2)?,
        publisher_name: row.get(3)?,
        language: row.get(4)?,
        total_resource_bytes: u64::try_from(total_resource_bytes).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Integer,
                Box::new(error),
            )
        })?,
        imported_at_unix_seconds: u64::try_from(imported_at).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Integer,
                Box::new(error),
            )
        })?,
    })
}

fn sqlite_integer(value: u64, name: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::IntegerOverflow(name))
}

#[cfg(unix)]
fn encode_path(path: &Path) -> (&'static str, Vec<u8>) {
    use std::os::unix::ffi::OsStrExt;
    ("unix", path.as_os_str().as_bytes().to_vec())
}

#[cfg(windows)]
fn encode_path(path: &Path) -> (&'static str, Vec<u8>) {
    use std::os::windows::ffi::OsStrExt;
    let bytes = path
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    ("windows-utf16le", bytes)
}

#[cfg(not(any(unix, windows)))]
fn encode_path(path: &Path) -> (&'static str, Vec<u8>) {
    ("utf8", path.to_string_lossy().into_owned().into_bytes())
}

#[cfg(unix)]
fn decode_path(encoding: &str, bytes: &[u8]) -> Result<PathBuf, StorageError> {
    use std::os::unix::ffi::OsStrExt;
    if encoding != "unix" {
        return Err(StorageError::MissingCapability("stored path encoding"));
    }
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(windows)]
fn decode_path(encoding: &str, bytes: &[u8]) -> Result<PathBuf, StorageError> {
    use std::os::windows::ffi::OsStringExt;
    if encoding != "windows-utf16le" || !bytes.len().is_multiple_of(2) {
        return Err(StorageError::MissingCapability("stored path encoding"));
    }
    let wide = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&wide)))
}

#[cfg(not(any(unix, windows)))]
fn decode_path(encoding: &str, bytes: &[u8]) -> Result<PathBuf, StorageError> {
    if encoding != "utf8" {
        return Err(StorageError::MissingCapability("stored path encoding"));
    }
    String::from_utf8(bytes.to_vec())
        .map(PathBuf::from)
        .map_err(|_| StorageError::MissingCapability("stored path encoding"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(title: &str, tag: &str) -> CourseManifest {
        let fixture = include_bytes!("../../course-format/tests/fixtures/valid-course.json");
        let mut manifest = course_format::parse_and_validate(fixture).unwrap();
        manifest.title = title.to_owned();
        manifest.tags = vec![tag.to_owned()];
        manifest
    }

    #[test]
    fn migrates_and_proves_fts5() {
        let storage = Storage::open_in_memory().unwrap();
        assert!(storage.list_courses().unwrap().is_empty());
    }

    #[test]
    fn saves_loads_and_replaces_course_atomically() {
        let mut storage = Storage::open_in_memory().unwrap();
        let original = manifest("Distributed Systems", "networks");
        storage
            .save_imported_course_at(&original, 128, Path::new("/course/original"), 10)
            .unwrap();
        assert_eq!(
            storage.course(&original.course_id).unwrap().unwrap().title,
            "Distributed Systems"
        );
        assert_eq!(
            storage.course_manifest(&original.course_id).unwrap(),
            Some(original.clone())
        );

        let updated = manifest("Distributed Systems Updated", "consensus");
        storage
            .save_imported_course_at(&updated, 256, Path::new("/course/updated"), 20)
            .unwrap();
        let summary = storage.course(&updated.course_id).unwrap().unwrap();
        assert_eq!(summary.title, "Distributed Systems Updated");
        assert_eq!(summary.total_resource_bytes, 256);
        assert_eq!(summary.imported_at_unix_seconds, 20);
        assert_eq!(
            storage.course_package_root(&updated.course_id).unwrap(),
            Some(PathBuf::from("/course/updated"))
        );
        assert!(storage.search_courses("networks").unwrap().is_empty());
        assert_eq!(storage.search_courses("consensus").unwrap().len(), 1);
    }

    #[test]
    fn rejects_future_schema_versions() {
        let connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 999).unwrap();
        assert!(matches!(
            Storage::initialize(connection),
            Err(StorageError::FutureSchema { found: 999, .. })
        ));
    }

    #[test]
    fn rejects_values_larger_than_sqlite_integer() {
        let mut storage = Storage::open_in_memory().unwrap();
        let manifest = manifest("Course", "tag");
        assert!(matches!(
            storage.save_imported_course_at(&manifest, u64::MAX, Path::new("/course"), 1),
            Err(StorageError::IntegerOverflow("total resource bytes"))
        ));
    }

    #[test]
    fn rejects_corrupt_stored_manifest_snapshots() {
        let mut storage = Storage::open_in_memory().unwrap();
        let manifest = manifest("Course", "tag");
        storage
            .save_imported_course_at(&manifest, 128, Path::new("/course"), 1)
            .unwrap();
        storage
            .connection
            .execute(
                "UPDATE courses SET manifest_json = ?1 WHERE course_id = ?2",
                params![br#"{"schemaVersion":1}"#, manifest.course_id],
            )
            .unwrap();
        assert!(matches!(
            storage.course_manifest(&manifest.course_id),
            Err(StorageError::Manifest(_))
        ));
    }
}
