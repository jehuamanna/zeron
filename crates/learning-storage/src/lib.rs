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

const CURRENT_SCHEMA_VERSION: i64 = 8;
const SUMMARY_COLUMNS: &str = "c.course_id, c.course_version, c.title, c.publisher_name, c.language, c.total_resource_bytes, c.imported_at, t.info_hash, coalesce(t.sharing, 0)";

/// What is recorded about one lesson.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LessonProgressRecord {
    /// Stable lesson ID from the manifest.
    pub lesson_id: String,
    /// Whether the lesson was completed.
    pub completed: bool,
}

/// Where a learner stopped in one lesson resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackPosition {
    /// Position in milliseconds.
    pub position_ms: u64,
    /// Media duration in milliseconds when the position was recorded.
    pub duration_ms: u64,
}

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
    /// Info hash when the course was imported from a torrent; such courses'
    /// resource bytes are not locally verified until downloaded.
    pub torrent_info_hash: Option<String>,
    /// Whether the learner chose to share this torrent-sourced course.
    pub sharing: bool,
}

/// Backend-only description of a torrent-sourced course for engine restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentCourseSource {
    /// Stable logical course ID.
    pub course_id: String,
    /// v1 info hash.
    pub info_hash: String,
    /// Complete `.torrent` bytes.
    pub metainfo: Vec<u8>,
    /// Learner-owned download directory. Never serialize this over IPC.
    pub content_root: PathBuf,
    /// Whether the learner chose to share this course with peers.
    pub sharing: bool,
    /// Whether the learner paused this course's transfer.
    pub paused: bool,
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
        self.save_imported_course_at(
            manifest,
            total_resource_bytes,
            package_root,
            imported_at,
            None,
        )
    }

    /// Persist a course whose validated manifest came from a torrent. Only
    /// `course.json` is verified at this point; resource bytes are not.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when serialization, clock, or `SQLite` operations fail.
    pub fn save_torrent_course(
        &mut self,
        manifest: &CourseManifest,
        total_resource_bytes: u64,
        content_root: &Path,
        info_hash: &str,
        metainfo: &[u8],
    ) -> Result<StoredCourseSummary, StorageError> {
        let imported_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StorageError::InvalidSystemClock)?
            .as_secs();
        self.save_imported_course_at(
            manifest,
            total_resource_bytes,
            content_root,
            imported_at,
            Some((info_hash, metainfo)),
        )
    }

    /// Torrent source of a course, if it was imported from a torrent.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures.
    pub fn course_torrent_info_hash(
        &self,
        course_id: &str,
    ) -> Result<Option<String>, StorageError> {
        self.connection
            .query_row(
                "SELECT info_hash FROM course_torrent_sources WHERE course_id = ?1",
                [course_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::from)
    }

    /// Every torrent-sourced course with its metainfo and download directory.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query or path-decoding failures.
    pub fn torrent_course_sources(&self) -> Result<Vec<TorrentCourseSource>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT t.course_id, t.info_hash, t.metainfo, l.path_encoding, l.path_bytes, t.sharing, t.paused
             FROM course_torrent_sources AS t JOIN course_locations AS l ON l.course_id = t.course_id
             ORDER BY t.course_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, bool>(5)?,
                row.get::<_, bool>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (course_id, info_hash, metainfo, encoding, bytes, sharing, paused) = row?;
            Ok(TorrentCourseSource {
                course_id,
                info_hash,
                metainfo,
                content_root: decode_path(&encoding, &bytes)?,
                sharing,
                paused,
            })
        })
        .collect()
    }

    /// Record whether the learner shares a torrent-sourced course. Returns
    /// `false` when the course is not torrent-sourced.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for write failures.
    pub fn set_course_sharing(
        &mut self,
        course_id: &str,
        sharing: bool,
    ) -> Result<bool, StorageError> {
        let changed = self.connection.execute(
            "UPDATE course_torrent_sources SET sharing = ?2 WHERE course_id = ?1",
            rusqlite::params![course_id, sharing],
        )?;
        Ok(changed == 1)
    }

    /// Remember where the learner stopped in a lesson resource.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for clock or `SQLite` failures, including an
    /// unknown course.
    pub fn set_playback_position(
        &mut self,
        course_id: &str,
        resource_id: &str,
        position: PlaybackPosition,
    ) -> Result<(), StorageError> {
        let updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StorageError::InvalidSystemClock)?
            .as_secs();
        self.connection.execute(
            "INSERT INTO playback_positions (course_id, resource_id, position_ms, duration_ms, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(course_id, resource_id) DO UPDATE SET
               position_ms = excluded.position_ms,
               duration_ms = excluded.duration_ms,
               updated_at = excluded.updated_at",
            params![
                course_id,
                resource_id,
                sqlite_integer(position.position_ms, "playback position")?,
                sqlite_integer(position.duration_ms, "playback duration")?,
                sqlite_integer(updated_at, "playback timestamp")?,
            ],
        )?;
        Ok(())
    }

    /// Where the learner stopped in a lesson resource, if recorded.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures or corrupt values.
    pub fn playback_position(
        &self,
        course_id: &str,
        resource_id: &str,
    ) -> Result<Option<PlaybackPosition>, StorageError> {
        let row: Option<(i64, i64)> = self
            .connection
            .query_row(
                "SELECT position_ms, duration_ms FROM playback_positions WHERE course_id = ?1 AND resource_id = ?2",
                [course_id, resource_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(position_ms, duration_ms)| {
            Ok(PlaybackPosition {
                position_ms: u64::try_from(position_ms)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, position_ms))?,
                duration_ms: u64::try_from(duration_ms)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, duration_ms))?,
            })
        })
        .transpose()
    }

    /// Add or remove a download commitment. `scope` is `course`, `module`, or
    /// `lesson`; for a course the scope ID is the course ID.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for clock or `SQLite` failures, including an
    /// unknown course or scope.
    pub fn set_download_commitment(
        &mut self,
        course_id: &str,
        scope: &str,
        scope_id: &str,
        committed: bool,
    ) -> Result<(), StorageError> {
        if committed {
            let created_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| StorageError::InvalidSystemClock)?
                .as_secs();
            self.connection.execute(
                "INSERT INTO download_commitments (course_id, scope, scope_id, created_at)
                 VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
                params![
                    course_id,
                    scope,
                    scope_id,
                    sqlite_integer(created_at, "commitment timestamp")?
                ],
            )?;
        } else {
            self.connection.execute(
                "DELETE FROM download_commitments WHERE course_id = ?1 AND scope = ?2 AND scope_id = ?3",
                params![course_id, scope, scope_id],
            )?;
        }
        Ok(())
    }

    /// A course's download commitments as `(scope, scope_id)` pairs.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures.
    pub fn download_commitments(
        &self,
        course_id: &str,
    ) -> Result<Vec<(String, String)>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT scope, scope_id FROM download_commitments WHERE course_id = ?1 ORDER BY created_at, scope, scope_id",
        )?;
        let rows = statement.query_map([course_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    /// Record whether the learner paused a torrent-sourced course. Returns
    /// `false` when the course is not torrent-sourced.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for write failures.
    pub fn set_course_paused(
        &mut self,
        course_id: &str,
        paused: bool,
    ) -> Result<bool, StorageError> {
        let changed = self.connection.execute(
            "UPDATE course_torrent_sources SET paused = ?2 WHERE course_id = ?1",
            rusqlite::params![course_id, paused],
        )?;
        Ok(changed == 1)
    }

    /// When each lesson resource of a course was last played, as Unix
    /// seconds, for resources with a recorded position.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures or corrupt values.
    pub fn playback_recency(
        &self,
        course_id: &str,
    ) -> Result<std::collections::HashMap<String, u64>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT resource_id, updated_at FROM playback_positions WHERE course_id = ?1",
        )?;
        let rows = statement.query_map([course_id], |row| {
            let updated_at: i64 = row.get(1)?;
            Ok((
                row.get::<_, String>(0)?,
                u64::try_from(updated_at)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, updated_at))?,
            ))
        })?;
        rows.collect::<Result<_, _>>().map_err(StorageError::from)
    }

    /// Record that a lesson was opened. Recording it again changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for clock or `SQLite` failures, including an
    /// unknown course.
    pub fn mark_lesson_started(
        &mut self,
        course_id: &str,
        lesson_id: &str,
    ) -> Result<(), StorageError> {
        let now = unix_now()?;
        self.connection.execute(
            "INSERT INTO lesson_progress (course_id, lesson_id, started_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(course_id, lesson_id) DO NOTHING",
            params![course_id, lesson_id, now],
        )?;
        Ok(())
    }

    /// Mark a lesson completed or not completed; either way it counts as started.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for clock or `SQLite` failures, including an
    /// unknown course.
    pub fn set_lesson_completed(
        &mut self,
        course_id: &str,
        lesson_id: &str,
        completed: bool,
    ) -> Result<(), StorageError> {
        let now = unix_now()?;
        self.connection.execute(
            "INSERT INTO lesson_progress (course_id, lesson_id, started_at, completed_at)
             VALUES (?1, ?2, ?3, CASE WHEN ?4 THEN ?3 END)
             ON CONFLICT(course_id, lesson_id) DO UPDATE SET
               completed_at = CASE WHEN ?4 THEN coalesce(lesson_progress.completed_at, ?3) END",
            params![course_id, lesson_id, now, completed],
        )?;
        Ok(())
    }

    /// Every recorded lesson of a course.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures.
    pub fn lesson_progress(
        &self,
        course_id: &str,
    ) -> Result<Vec<LessonProgressRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT lesson_id, completed_at IS NOT NULL FROM lesson_progress WHERE course_id = ?1 ORDER BY lesson_id",
        )?;
        let rows = statement.query_map([course_id], |row| {
            Ok(LessonProgressRecord {
                lesson_id: row.get(0)?,
                completed: row.get(1)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(StorageError::from)
    }

    /// Read a learner setting.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures.
    pub fn setting(&self, key: &str) -> Result<Option<String>, StorageError> {
        self.connection
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(StorageError::from)
    }

    /// Write a learner setting.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for `SQLite` failures.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
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
                &format!("SELECT {SUMMARY_COLUMNS} FROM courses AS c LEFT JOIN course_torrent_sources AS t ON t.course_id = c.course_id WHERE c.course_id = ?1"),
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
        let mut statement = self.connection.prepare(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM courses AS c LEFT JOIN course_torrent_sources AS t ON t.course_id = c.course_id ORDER BY c.title COLLATE NOCASE, c.course_id"
        ))?;
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
        let mut statement = self.connection.prepare(&format!(
            "SELECT {SUMMARY_COLUMNS}
             FROM course_search AS s JOIN courses AS c ON c.course_id = s.course_id
             LEFT JOIN course_torrent_sources AS t ON t.course_id = c.course_id
             WHERE course_search MATCH ?1
             ORDER BY bm25(course_search), c.course_id"
        ))?;
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
        torrent: Option<(&str, &[u8])>,
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
        tx.execute(
            "DELETE FROM course_torrent_sources WHERE course_id = ?1",
            [&manifest.course_id],
        )?;
        if let Some((info_hash, metainfo)) = torrent {
            tx.execute(
                "INSERT INTO course_torrent_sources (course_id, info_hash, metainfo) VALUES (?1, ?2, ?3)",
                params![manifest.course_id, info_hash, metainfo],
            )?;
        }
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
            torrent_info_hash: torrent.map(|(info_hash, _)| info_hash.to_owned()),
            // Every import replaces the source row, so sharing starts off again.
            sharing: false,
        })
    }
}

/// One schema step, upgrading by exactly one version.
type Migration = fn(&mut Connection) -> Result<(), StorageError>;

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
        found = 2;
    }
    if found == 2 {
        let tx = connection.transaction()?;
        tx.execute_batch(
            "CREATE TABLE course_torrent_sources (
               course_id TEXT PRIMARY KEY NOT NULL REFERENCES courses(course_id) ON DELETE CASCADE,
               info_hash TEXT NOT NULL,
               metainfo BLOB NOT NULL
             ) STRICT;
             CREATE TABLE settings (
               key TEXT PRIMARY KEY NOT NULL,
               value TEXT NOT NULL
             ) STRICT;
             PRAGMA user_version = 3;",
        )?;
        tx.commit()?;
        found = 3;
    }
    if found == 3 {
        let tx = connection.transaction()?;
        tx.execute_batch(
            "ALTER TABLE course_torrent_sources
               ADD COLUMN sharing INTEGER NOT NULL DEFAULT 0 CHECK(sharing IN (0, 1));
             PRAGMA user_version = 4;",
        )?;
        tx.commit()?;
        found = 4;
    }
    // Later versions each add one table or column; they run in order.
    let steps: [(i64, Migration); 4] = [
        (4, add_playback_positions),
        (5, add_download_commitments),
        (6, add_pause_flag),
        (7, add_lesson_progress),
    ];
    for (from, step) in steps {
        if found == from {
            step(connection)?;
            found += 1;
        }
    }
    Ok(())
}

/// Schema 5. Keyed by course only, so positions survive a re-import that
/// rewrites `course_resources`; callers check the resource exists.
fn add_playback_positions(connection: &mut Connection) -> Result<(), StorageError> {
    let tx = connection.transaction()?;
    tx.execute_batch(
        "CREATE TABLE playback_positions (
           course_id TEXT NOT NULL REFERENCES courses(course_id) ON DELETE CASCADE,
           resource_id TEXT NOT NULL,
           position_ms INTEGER NOT NULL CHECK(position_ms >= 0),
           duration_ms INTEGER NOT NULL CHECK(duration_ms >= 0),
           updated_at INTEGER NOT NULL CHECK(updated_at >= 0),
           PRIMARY KEY(course_id, resource_id)
         ) STRICT;
         PRAGMA user_version = 5;",
    )?;
    tx.commit()?;
    Ok(())
}

/// Schema 6. What the learner asked to keep offline, by course, module, or
/// lesson. Keyed by course only, like playback positions.
fn add_download_commitments(connection: &mut Connection) -> Result<(), StorageError> {
    let tx = connection.transaction()?;
    tx.execute_batch(
        "CREATE TABLE download_commitments (
           course_id TEXT NOT NULL REFERENCES courses(course_id) ON DELETE CASCADE,
           scope TEXT NOT NULL CHECK(scope IN ('course', 'module', 'lesson')),
           scope_id TEXT NOT NULL,
           created_at INTEGER NOT NULL CHECK(created_at >= 0),
           PRIMARY KEY(course_id, scope, scope_id)
         ) STRICT;
         PRAGMA user_version = 6;",
    )?;
    tx.commit()?;
    Ok(())
}

/// Schema 7. Whether the learner paused a torrent course's transfer.
fn add_pause_flag(connection: &mut Connection) -> Result<(), StorageError> {
    let tx = connection.transaction()?;
    tx.execute_batch(
        "ALTER TABLE course_torrent_sources
           ADD COLUMN paused INTEGER NOT NULL DEFAULT 0 CHECK(paused IN (0, 1));
         PRAGMA user_version = 7;",
    )?;
    tx.commit()?;
    Ok(())
}

/// Schema 8. When each lesson was first opened and, if so, completed. Keyed
/// by course only, so a re-import keeps the learner's progress.
fn add_lesson_progress(connection: &mut Connection) -> Result<(), StorageError> {
    let tx = connection.transaction()?;
    tx.execute_batch(
        "CREATE TABLE lesson_progress (
           course_id TEXT NOT NULL REFERENCES courses(course_id) ON DELETE CASCADE,
           lesson_id TEXT NOT NULL,
           started_at INTEGER NOT NULL CHECK(started_at >= 0),
           completed_at INTEGER CHECK(completed_at IS NULL OR completed_at >= 0),
           PRIMARY KEY(course_id, lesson_id)
         ) STRICT;
         PRAGMA user_version = 8;",
    )?;
    tx.commit()?;
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
        torrent_info_hash: row.get(7)?,
        sharing: row.get(8)?,
    })
}

fn unix_now() -> Result<i64, StorageError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::InvalidSystemClock)?
        .as_secs();
    sqlite_integer(seconds, "timestamp")
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
        assert_eq!(storage.list_courses().unwrap().len(), 0);
    }

    #[test]
    fn saves_loads_and_replaces_course_atomically() {
        let mut storage = Storage::open_in_memory().unwrap();
        let original = manifest("Distributed Systems", "networks");
        storage
            .save_imported_course_at(&original, 128, Path::new("/course/original"), 10, None)
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
            .save_imported_course_at(&updated, 256, Path::new("/course/updated"), 20, None)
            .unwrap();
        let summary = storage.course(&updated.course_id).unwrap().unwrap();
        assert_eq!(summary.title, "Distributed Systems Updated");
        assert_eq!(summary.total_resource_bytes, 256);
        assert_eq!(summary.imported_at_unix_seconds, 20);
        assert_eq!(
            storage.course_package_root(&updated.course_id).unwrap(),
            Some(PathBuf::from("/course/updated"))
        );
        assert_eq!(storage.search_courses("networks").unwrap().len(), 0);
        assert_eq!(storage.search_courses("consensus").unwrap().len(), 1);
    }

    #[test]
    fn torrent_sources_are_recorded_and_cleared_by_local_reimport() {
        let mut storage = Storage::open_in_memory().unwrap();
        let course = manifest("Torrent Course", "p2p");
        let summary = storage
            .save_torrent_course(
                &course,
                64,
                Path::new("/cache/abc"),
                &"a".repeat(40),
                b"d4:infoe",
            )
            .unwrap();
        assert_eq!(
            summary.torrent_info_hash.as_deref(),
            Some("a".repeat(40).as_str())
        );
        assert_eq!(
            storage.list_courses().unwrap()[0]
                .torrent_info_hash
                .as_deref(),
            Some("a".repeat(40).as_str())
        );
        assert_eq!(
            storage.course_torrent_info_hash(&course.course_id).unwrap(),
            Some("a".repeat(40))
        );
        let sources = storage.torrent_course_sources().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].metainfo, b"d4:infoe");
        assert_eq!(sources[0].content_root, Path::new("/cache/abc"));
        assert!(
            !sources[0].sharing,
            "sharing is off until the learner opts in"
        );
        assert!(storage.set_course_sharing(&course.course_id, true).unwrap());
        assert!(storage.torrent_course_sources().unwrap()[0].sharing);
        assert!(storage.list_courses().unwrap()[0].sharing);
        assert!(!storage.set_course_sharing("unknown", true).unwrap());
        assert!(!storage.torrent_course_sources().unwrap()[0].paused);
        assert!(storage.set_course_paused(&course.course_id, true).unwrap());
        assert!(storage.torrent_course_sources().unwrap()[0].paused);
        assert!(!storage.set_course_paused("unknown", true).unwrap());

        storage
            .save_imported_course(&course, 64, Path::new("/local/course"))
            .unwrap();
        assert!(
            storage
                .course(&course.course_id)
                .unwrap()
                .unwrap()
                .torrent_info_hash
                .is_none()
        );
        assert_eq!(storage.torrent_course_sources().unwrap().len(), 0);
        assert!(!storage.list_courses().unwrap()[0].sharing);
        assert!(
            !storage.set_course_sharing(&course.course_id, true).unwrap(),
            "local courses cannot be shared"
        );

        assert_eq!(storage.setting("network_profile").unwrap(), None);
        storage.set_setting("network_profile", "internet").unwrap();
        assert_eq!(
            storage.setting("network_profile").unwrap().as_deref(),
            Some("internet")
        );
    }

    #[test]
    fn playback_positions_are_upserted_and_survive_a_reimport() {
        let mut storage = Storage::open_in_memory().unwrap();
        let course = manifest("Rust", "systems");
        let course_id = course.course_id.clone();
        let position = |position_ms, duration_ms| PlaybackPosition {
            position_ms,
            duration_ms,
        };
        assert!(
            storage
                .set_playback_position(&course_id, "res", position(1, 2))
                .is_err()
        );
        storage
            .save_imported_course(&course, 10, Path::new("/courses/rust"))
            .unwrap();
        assert_eq!(storage.playback_position(&course_id, "res").unwrap(), None);
        storage
            .set_playback_position(&course_id, "res", position(61_500, 600_000))
            .unwrap();
        storage
            .set_playback_position(&course_id, "res", position(90_000, 600_000))
            .unwrap();
        storage
            .set_playback_position(&course_id, "other", position(5_000, 10_000))
            .unwrap();
        assert_eq!(
            storage.playback_position(&course_id, "res").unwrap(),
            Some(position(90_000, 600_000))
        );
        storage
            .save_imported_course(&course, 10, Path::new("/courses/rust"))
            .unwrap();
        assert_eq!(
            storage.playback_position(&course_id, "res").unwrap(),
            Some(position(90_000, 600_000))
        );
        assert!(
            storage
                .set_playback_position(&course_id, "res", position(u64::MAX, 1))
                .is_err()
        );
        let recency = storage.playback_recency(&course_id).unwrap();
        assert_eq!(recency.len(), 2);
        assert!(recency["res"] > 0);
        assert!(
            storage
                .playback_recency("course_missing")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn version_four_databases_gain_playback_positions() {
        let path =
            std::env::temp_dir().join(format!("zeron-learning-v4-{}.sqlite3", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let course = manifest("Rust", "systems");
        {
            let mut storage = Storage::open(&path).unwrap();
            storage
                .save_imported_course(&course, 10, Path::new("/courses/rust"))
                .unwrap();
            storage
                .connection
                .execute_batch(
                    "DROP TABLE lesson_progress;
                     ALTER TABLE course_torrent_sources DROP COLUMN paused;
                     DROP TABLE download_commitments; DROP TABLE playback_positions;
                     PRAGMA user_version = 4;",
                )
                .unwrap();
        }
        let mut storage = Storage::open(&path).unwrap();
        assert_eq!(storage.list_courses().unwrap().len(), 1);
        let position = PlaybackPosition {
            position_ms: 1_000,
            duration_ms: 2_000,
        };
        storage
            .set_playback_position(&course.course_id, "res", position)
            .unwrap();
        assert_eq!(
            storage.playback_position(&course.course_id, "res").unwrap(),
            Some(position)
        );
        drop(storage);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn download_commitments_are_idempotent_and_scoped_to_known_courses() {
        let mut storage = Storage::open_in_memory().unwrap();
        let course = manifest("Rust", "systems");
        let id = course.course_id.clone();
        assert!(
            storage
                .set_download_commitment(&id, "lesson", "l1", true)
                .is_err()
        );
        storage
            .save_imported_course(&course, 10, Path::new("/courses/rust"))
            .unwrap();
        assert!(
            storage
                .set_download_commitment(&id, "chapter", "x", true)
                .is_err()
        );
        storage
            .set_download_commitment(&id, "lesson", "l1", true)
            .unwrap();
        storage
            .set_download_commitment(&id, "lesson", "l1", true)
            .unwrap();
        storage
            .set_download_commitment(&id, "module", "m1", true)
            .unwrap();
        assert_eq!(
            storage.download_commitments(&id).unwrap(),
            [
                ("lesson".to_owned(), "l1".to_owned()),
                ("module".to_owned(), "m1".to_owned())
            ]
        );
        storage
            .set_download_commitment(&id, "lesson", "l1", false)
            .unwrap();
        storage
            .set_download_commitment(&id, "lesson", "unknown", false)
            .unwrap();
        storage
            .save_imported_course(&course, 10, Path::new("/courses/rust"))
            .unwrap();
        assert_eq!(
            storage.download_commitments(&id).unwrap(),
            [("module".to_owned(), "m1".to_owned())]
        );
    }

    #[test]
    fn lesson_progress_records_starts_and_keeps_the_first_completion() {
        let mut storage = Storage::open_in_memory().unwrap();
        let course = manifest("Rust", "systems");
        let id = course.course_id.clone();
        assert!(storage.mark_lesson_started(&id, "l1").is_err());
        storage
            .save_imported_course(&course, 10, Path::new("/courses/rust"))
            .unwrap();
        storage.mark_lesson_started(&id, "l1").unwrap();
        storage.mark_lesson_started(&id, "l1").unwrap();
        storage.set_lesson_completed(&id, "l2", true).unwrap();
        let record = |lesson: &str, completed| LessonProgressRecord {
            lesson_id: lesson.to_owned(),
            completed,
        };
        assert_eq!(
            storage.lesson_progress(&id).unwrap(),
            [record("l1", false), record("l2", true)]
        );
        storage.set_lesson_completed(&id, "l2", true).unwrap();
        storage.set_lesson_completed(&id, "l1", true).unwrap();
        storage.set_lesson_completed(&id, "l2", false).unwrap();
        storage
            .save_imported_course(&course, 10, Path::new("/courses/rust"))
            .unwrap();
        assert_eq!(
            storage.lesson_progress(&id).unwrap(),
            [record("l1", true), record("l2", false)]
        );
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
            storage.save_imported_course_at(&manifest, u64::MAX, Path::new("/course"), 1, None),
            Err(StorageError::IntegerOverflow("total resource bytes"))
        ));
    }

    #[test]
    fn rejects_corrupt_stored_manifest_snapshots() {
        let mut storage = Storage::open_in_memory().unwrap();
        let manifest = manifest("Course", "tag");
        storage
            .save_imported_course_at(&manifest, 128, Path::new("/course"), 1, None)
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
