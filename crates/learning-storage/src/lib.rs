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

/// Schema version this build creates and understands.
pub const CURRENT_SCHEMA_VERSION: i64 = 14;
/// Previous versions of one course whose manifests are kept for moving annotations.
pub const KEPT_PREVIOUS_MANIFESTS: u32 = 5;
const SUMMARY_COLUMNS: &str = "c.course_id, c.course_version, c.title, c.publisher_name, c.language, c.total_resource_bytes, c.imported_at, t.info_hash, coalesce(t.sharing, 0)";

/// A publisher catalog kept by the Library. `document` is the verified
/// catalog file, kept as it arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCatalog {
    /// The publisher's identity.
    pub publisher_id: String,
    /// Title shown to learners.
    pub title: String,
    /// When the publisher issued it, in Unix seconds.
    pub issued_at: u64,
    /// HTTPS address it was fetched from, if any.
    pub source_url: Option<String>,
    /// The catalog file.
    pub document: Vec<u8>,
}

/// A verified publisher signature of a course version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseSignature {
    /// The publisher's identity (root key).
    pub publisher_id: String,
    /// The key that signed this version.
    pub signing_key: String,
    /// When it was signed, in Unix seconds.
    pub signed_at: u64,
}

/// What a save records about who published the course.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CourseTrust {
    /// The verified signature of the version being saved, if it is signed.
    pub signature: Option<CourseSignature>,
    /// Pin the course to this publisher if it has no pin yet.
    pub pin: Option<String>,
}

/// Annotations to move from one lesson to another of the same course.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationMoveRequest {
    /// Course both lessons belong to.
    pub course_id: String,
    /// Lesson the annotations are on now.
    pub from_lesson_id: String,
    /// Lesson they move to; it may equal `from_lesson_id` to repoint anchors.
    pub to_lesson_id: String,
    /// Each note of the lesson to move, with its anchor for the target.
    pub note_anchors: Vec<(String, String)>,
    /// Old resource ID to new, for playback positions.
    pub resource_map: Vec<(String, String)>,
}

/// What a move changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnnotationMoveOutcome {
    /// Notes moved or repointed.
    pub notes_moved: usize,
    /// The drawing moved to the target lesson.
    pub drawing_moved: bool,
    /// The target already had a drawing, so this one stayed where it was.
    pub drawing_kept: bool,
    /// Lesson progress moved; it stays when the target already has its own.
    pub progress_moved: bool,
    /// Playback positions moved to the mapped resources.
    pub positions_moved: usize,
}

/// A stored learner note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRecord {
    /// Opaque note ID.
    pub note_id: String,
    /// Course the note belongs to.
    pub course_id: String,
    /// Lesson the note belongs to.
    pub lesson_id: String,
    /// Serialized anchor.
    pub anchor_json: String,
    /// Serialized note document.
    pub body_json: String,
    /// Plain text of the document.
    pub body_text: String,
    /// Increases with every saved change.
    pub revision: u64,
    /// Creation time in Unix seconds.
    pub created_at: u64,
    /// Last change in Unix seconds.
    pub updated_at: u64,
}

/// Outcome of a compare-and-swap note write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteWrite {
    /// Saved; carries the stored note (for deletes, the note as it was).
    Saved(NoteRecord),
    /// The note changed since the caller read it.
    Conflict {
        /// Current revision.
        current_revision: u64,
    },
    /// No such note.
    Missing,
}

/// A passage of a course file to index for search.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchEntry {
    /// Lesson the passage belongs to.
    pub lesson_id: String,
    /// `transcript` or `text`.
    pub kind: &'static str,
    /// What a result opens: the media for a transcript, else the resource.
    pub source_id: String,
    /// Where in the media the passage starts, in seconds.
    pub position: Option<f64>,
    /// The passage.
    pub body: String,
}

/// One search match.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// Course of the match.
    pub course_id: String,
    /// Lesson of the match; empty for the course itself.
    pub lesson_id: String,
    /// `course`, `lesson`, `note`, `transcript`, or `text`.
    pub kind: String,
    /// Course, lesson, note, or resource the match opens.
    pub source_id: String,
    /// Seconds into the media, for transcripts.
    pub position: Option<f64>,
    /// Matching excerpt; matched terms are wrapped in U+E000 and U+E001.
    pub snippet: String,
}

/// A stored lesson drawing with its images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingRecord {
    /// Serialized Excalidraw scene.
    pub scene_json: String,
    /// Increases with every save; saves must name it.
    pub revision: u64,
    /// `(file_id, mime_type, data_url)` of every stored image.
    pub files: Vec<(String, String, String)>,
}

/// Outcome of a compare-and-swap drawing save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawingWrite {
    /// Saved at this revision.
    Saved {
        /// The new revision.
        revision: u64,
    },
    /// Another save got there first; nothing was written.
    Conflict {
        /// Revision to reload.
        current_revision: u64,
    },
}

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

    /// [`Self::save_imported_course`], also recording who published it.
    ///
    /// # Errors
    ///
    /// As [`Self::save_imported_course`].
    pub fn save_imported_course_trusted(
        &mut self,
        manifest: &CourseManifest,
        total_resource_bytes: u64,
        package_root: &Path,
        trust: &CourseTrust,
    ) -> Result<StoredCourseSummary, StorageError> {
        let imported_at = unix_now_u64()?;
        self.save_course(
            manifest,
            total_resource_bytes,
            package_root,
            imported_at,
            None,
            trust,
        )
    }

    /// [`Self::save_torrent_course`], also recording who published it.
    ///
    /// # Errors
    ///
    /// As [`Self::save_torrent_course`].
    pub fn save_torrent_course_trusted(
        &mut self,
        manifest: &CourseManifest,
        total_resource_bytes: u64,
        content_root: &Path,
        torrent: (&str, &[u8]),
        trust: &CourseTrust,
    ) -> Result<StoredCourseSummary, StorageError> {
        let imported_at = unix_now_u64()?;
        self.save_course(
            manifest,
            total_resource_bytes,
            content_root,
            imported_at,
            Some(torrent),
            trust,
        )
    }

    /// The publisher a course is pinned to, if any.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures.
    pub fn course_pin(&self, course_id: &str) -> Result<Option<String>, StorageError> {
        Ok(self
            .connection
            .query_row(
                "SELECT publisher_id FROM course_pins WHERE course_id = ?1",
                [course_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Forget a course's pinned publisher, so its next signed import pins
    /// again. Returns whether there was a pin.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for write failures.
    pub fn forget_course_pin(&mut self, course_id: &str) -> Result<bool, StorageError> {
        Ok(self
            .connection
            .execute("DELETE FROM course_pins WHERE course_id = ?1", [course_id])?
            > 0)
    }

    /// Keep a publisher's catalog, replacing the one it had.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for clock or write failures.
    pub fn save_catalog(&mut self, catalog: &StoredCatalog) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO catalogs (publisher_id, title, issued_at, source_url, document, imported_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(publisher_id) DO UPDATE SET
               title = excluded.title, issued_at = excluded.issued_at,
               source_url = excluded.source_url, document = excluded.document,
               imported_at = excluded.imported_at",
            params![
                catalog.publisher_id,
                catalog.title,
                sqlite_integer(catalog.issued_at, "catalog issue time")?,
                catalog.source_url,
                catalog.document,
                unix_now()?,
            ],
        )?;
        Ok(())
    }

    /// Every kept catalog, by title.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures.
    pub fn catalogs(&self) -> Result<Vec<StoredCatalog>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT publisher_id, title, issued_at, source_url, document FROM catalogs
             ORDER BY title, publisher_id",
        )?;
        let rows = statement.query_map([], |row| {
            let issued_at: i64 = row.get(2)?;
            Ok(StoredCatalog {
                publisher_id: row.get(0)?,
                title: row.get(1)?,
                issued_at: u64::try_from(issued_at)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, issued_at))?,
                source_url: row.get(3)?,
                document: row.get(4)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(StorageError::from)
    }

    /// Forget a catalog. Courses imported from it stay. Returns whether one
    /// was kept.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for write failures.
    pub fn remove_catalog(&mut self, publisher_id: &str) -> Result<bool, StorageError> {
        Ok(self.connection.execute(
            "DELETE FROM catalogs WHERE publisher_id = ?1",
            [publisher_id],
        )? > 0)
    }

    /// Signed key revocations, as JSON statements, oldest first. Callers
    /// verify them; storage only keeps them.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures.
    pub fn revocation_statements(&self) -> Result<Vec<String>, StorageError> {
        let mut statement = self
            .connection
            .prepare("SELECT statement_json FROM key_revocations ORDER BY recorded_at, rowid")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<Result<_, _>>().map_err(StorageError::from)
    }

    /// Keep a signed key revocation; an identical one is kept once.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for clock or write failures.
    pub fn add_revocation_statement(&mut self, statement_json: &str) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO key_revocations (statement_json, recorded_at) VALUES (?1, ?2)
             ON CONFLICT(statement_json) DO NOTHING",
            params![statement_json, unix_now()?],
        )?;
        Ok(())
    }

    /// The verified signature of a course's current version, if it is signed.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query or decoding failures.
    pub fn course_signature(
        &self,
        course_id: &str,
    ) -> Result<Option<CourseSignature>, StorageError> {
        self.connection
            .query_row(
                "SELECT publisher_id, signing_key, signed_at FROM course_signatures WHERE course_id = ?1",
                [course_id],
                |row| {
                    let signed_at: i64 = row.get(2)?;
                    Ok(CourseSignature {
                        publisher_id: row.get(0)?,
                        signing_key: row.get(1)?,
                        signed_at: u64::try_from(signed_at)
                            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, signed_at))?,
                    })
                },
            )
            .optional()
            .map_err(StorageError::from)
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

    /// Store a new note at revision 0.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for clock or `SQLite` failures, including an
    /// unknown course or a reused note ID.
    pub fn insert_note(
        &mut self,
        note_id: &str,
        course_id: &str,
        lesson_id: &str,
        anchor_json: &str,
        body_json: &str,
        body_text: &str,
    ) -> Result<NoteRecord, StorageError> {
        let now = unix_now()?;
        self.connection.execute(
            "INSERT INTO notes (note_id, course_id, lesson_id, anchor_json, body_json, body_text, revision, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?7)",
            params![note_id, course_id, lesson_id, anchor_json, body_json, body_text, now],
        )?;
        self.note(note_id)?
            .ok_or(StorageError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
    }

    /// Replace a note's anchor and body only if it is still at
    /// `expected_revision`; the revision then advances by one.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for clock or `SQLite` failures.
    pub fn update_note(
        &mut self,
        note_id: &str,
        expected_revision: u64,
        anchor_json: &str,
        body_json: &str,
        body_text: &str,
    ) -> Result<NoteWrite, StorageError> {
        let now = unix_now()?;
        let changed = self.connection.execute(
            "UPDATE notes SET anchor_json = ?3, body_json = ?4, body_text = ?5, revision = revision + 1, updated_at = ?6
             WHERE note_id = ?1 AND revision = ?2",
            params![
                note_id,
                sqlite_integer(expected_revision, "note revision")?,
                anchor_json,
                body_json,
                body_text,
                now
            ],
        )?;
        self.note_write(note_id, changed)
    }

    /// Delete a note only if it is still at `expected_revision`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for `SQLite` failures.
    pub fn delete_note(
        &mut self,
        note_id: &str,
        expected_revision: u64,
    ) -> Result<NoteWrite, StorageError> {
        let Some(existing) = self.note(note_id)? else {
            return Ok(NoteWrite::Missing);
        };
        let deleted = self.connection.execute(
            "DELETE FROM notes WHERE note_id = ?1 AND revision = ?2",
            params![note_id, sqlite_integer(expected_revision, "note revision")?],
        )?;
        Ok(if deleted == 1 {
            NoteWrite::Saved(existing)
        } else {
            NoteWrite::Conflict {
                current_revision: existing.revision,
            }
        })
    }

    /// One note.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures.
    pub fn note(&self, note_id: &str) -> Result<Option<NoteRecord>, StorageError> {
        Ok(self
            .query_notes("WHERE note_id = ?1", [note_id])?
            .into_iter()
            .next())
    }

    /// Notes of a course, or of one of its lessons, oldest first.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures.
    pub fn notes(
        &self,
        course_id: &str,
        lesson_id: Option<&str>,
    ) -> Result<Vec<NoteRecord>, StorageError> {
        match lesson_id {
            Some(lesson_id) => self.query_notes(
                "WHERE course_id = ?1 AND lesson_id = ?2 ORDER BY created_at, rowid",
                [course_id, lesson_id],
            ),
            None => self.query_notes(
                "WHERE course_id = ?1 ORDER BY lesson_id, created_at, rowid",
                [course_id],
            ),
        }
    }

    fn note_write(&self, note_id: &str, changed: usize) -> Result<NoteWrite, StorageError> {
        Ok(match (changed, self.note(note_id)?) {
            (_, None) => NoteWrite::Missing,
            (1, Some(note)) => NoteWrite::Saved(note),
            (_, Some(note)) => NoteWrite::Conflict {
                current_revision: note.revision,
            },
        })
    }

    fn query_notes(
        &self,
        filter: &str,
        parameters: impl rusqlite::Params,
    ) -> Result<Vec<NoteRecord>, StorageError> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT note_id, course_id, lesson_id, anchor_json, body_json, body_text, revision, created_at, updated_at
             FROM notes {filter}"
        ))?;
        let rows = statement.query_map(parameters, |row| {
            let unsigned = |index: usize| -> rusqlite::Result<u64> {
                let value: i64 = row.get(index)?;
                u64::try_from(value)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
            };
            Ok(NoteRecord {
                note_id: row.get(0)?,
                course_id: row.get(1)?,
                lesson_id: row.get(2)?,
                anchor_json: row.get(3)?,
                body_json: row.get(4)?,
                body_text: row.get(5)?,
                revision: unsigned(6)?,
                created_at: unsigned(7)?,
                updated_at: unsigned(8)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(StorageError::from)
    }

    /// The drawing of a lesson, if any.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures.
    pub fn drawing(
        &self,
        course_id: &str,
        lesson_id: &str,
    ) -> Result<Option<DrawingRecord>, StorageError> {
        let Some((scene_json, revision)) = self
            .connection
            .query_row(
                "SELECT scene_json, revision FROM lesson_drawings WHERE course_id = ?1 AND lesson_id = ?2",
                [course_id, lesson_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let mut statement = self.connection.prepare(
            "SELECT file_id, mime_type, data_url FROM drawing_files WHERE course_id = ?1 AND lesson_id = ?2 ORDER BY created_at, file_id",
        )?;
        let files = statement
            .query_map([course_id, lesson_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(DrawingRecord {
            scene_json,
            revision: u64::try_from(revision)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, revision))?,
            files,
        }))
    }

    /// Save a lesson's drawing and any new images in one transaction:
    /// create it when `expected_revision` is `None`, otherwise replace it
    /// only if it is still at that revision. Images already stored under the
    /// same ID are kept as they are.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for clock or `SQLite` failures, including an
    /// unknown course.
    pub fn save_drawing(
        &mut self,
        course_id: &str,
        lesson_id: &str,
        expected_revision: Option<u64>,
        scene_json: &str,
        new_files: &[(String, String, String)],
    ) -> Result<DrawingWrite, StorageError> {
        let now = unix_now()?;
        let tx = self.connection.transaction()?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT revision FROM lesson_drawings WHERE course_id = ?1 AND lesson_id = ?2",
                [course_id, lesson_id],
                |row| row.get(0),
            )
            .optional()?;
        let current = current
            .map(|revision| {
                u64::try_from(revision)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, revision))
            })
            .transpose()?;
        let revision = match (current, expected_revision) {
            (None, None) => {
                tx.execute(
                    "INSERT INTO lesson_drawings (course_id, lesson_id, scene_json, revision, updated_at) VALUES (?1, ?2, ?3, 0, ?4)",
                    params![course_id, lesson_id, scene_json, now],
                )?;
                0
            }
            (Some(current), Some(expected)) if current == expected => {
                tx.execute(
                    "UPDATE lesson_drawings SET scene_json = ?3, revision = revision + 1, updated_at = ?4 WHERE course_id = ?1 AND lesson_id = ?2",
                    params![course_id, lesson_id, scene_json, now],
                )?;
                current + 1
            }
            (current, _) => {
                return Ok(DrawingWrite::Conflict {
                    current_revision: current.unwrap_or(0),
                });
            }
        };
        for (file_id, mime_type, data_url) in new_files {
            tx.execute(
                "INSERT INTO drawing_files (course_id, lesson_id, file_id, mime_type, data_url, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT DO NOTHING",
                params![course_id, lesson_id, file_id, mime_type, data_url, now],
            )?;
        }
        tx.commit()?;
        Ok(DrawingWrite::Saved { revision })
    }

    /// Manifests of the versions this course replaced, newest first.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query or decoding failures.
    pub fn previous_manifests(&self, course_id: &str) -> Result<Vec<CourseManifest>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT manifest_json FROM course_manifest_history WHERE course_id = ?1
             ORDER BY replaced_at DESC, rowid DESC",
        )?;
        let rows = statement.query_map([course_id], |row| row.get::<_, Vec<u8>>(0))?;
        rows.map(|json| Ok(serde_json::from_slice(&json?)?))
            .collect()
    }

    /// Lessons of a course that have a drawing.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures.
    pub fn drawing_lessons(&self, course_id: &str) -> Result<Vec<String>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT lesson_id FROM lesson_drawings WHERE course_id = ?1 ORDER BY lesson_id",
        )?;
        let rows = statement.query_map([course_id], |row| row.get(0))?;
        rows.collect::<Result<_, _>>().map_err(StorageError::from)
    }

    /// Move a lesson's annotations to another lesson of the same course in
    /// one transaction. Notes move with the anchors given for them, and each
    /// moved note's revision increases, so an editor holding the old one
    /// reloads rather than overwrites. The drawing, lesson progress, and the
    /// playback positions of mapped resources move only where the target
    /// has none, so nothing the target already has is overwritten.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for clock or `SQLite` failures.
    pub fn move_lesson_annotations(
        &mut self,
        request: &AnnotationMoveRequest,
    ) -> Result<AnnotationMoveOutcome, StorageError> {
        let now = unix_now()?;
        let tx = self.connection.transaction()?;
        let course = &request.course_id;
        let (from, to) = (&request.from_lesson_id, &request.to_lesson_id);
        let mut outcome = AnnotationMoveOutcome::default();
        for (note_id, anchor_json) in &request.note_anchors {
            outcome.notes_moved += tx.execute(
                "UPDATE notes SET lesson_id = ?4, anchor_json = ?5, revision = revision + 1, updated_at = ?6
                 WHERE note_id = ?1 AND course_id = ?2 AND lesson_id = ?3",
                params![note_id, course, from, to, anchor_json, now],
            )?;
        }
        if from != to {
            let exists = |table: &str, lesson: &str| -> Result<bool, StorageError> {
                Ok(tx.query_row(
                    &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE course_id = ?1 AND lesson_id = ?2)"),
                    [course, lesson],
                    |row| row.get(0),
                )?)
            };
            if exists("lesson_drawings", from)? {
                if exists("lesson_drawings", to)? {
                    outcome.drawing_kept = true;
                } else {
                    tx.execute(
                        "INSERT INTO lesson_drawings (course_id, lesson_id, scene_json, revision, updated_at)
                         SELECT course_id, ?3, scene_json, revision + 1, ?4 FROM lesson_drawings
                         WHERE course_id = ?1 AND lesson_id = ?2",
                        params![course, from, to, now],
                    )?;
                    tx.execute(
                        "UPDATE drawing_files SET lesson_id = ?3 WHERE course_id = ?1 AND lesson_id = ?2",
                        params![course, from, to],
                    )?;
                    tx.execute(
                        "DELETE FROM lesson_drawings WHERE course_id = ?1 AND lesson_id = ?2",
                        params![course, from],
                    )?;
                    outcome.drawing_moved = true;
                }
            }
            if exists("lesson_progress", from)? && !exists("lesson_progress", to)? {
                tx.execute(
                    "UPDATE lesson_progress SET lesson_id = ?3 WHERE course_id = ?1 AND lesson_id = ?2",
                    params![course, from, to],
                )?;
                outcome.progress_moved = true;
            }
        }
        for (old_resource, new_resource) in &request.resource_map {
            if old_resource == new_resource {
                continue;
            }
            outcome.positions_moved += tx.execute(
                "UPDATE playback_positions SET resource_id = ?3
                 WHERE course_id = ?1 AND resource_id = ?2 AND NOT EXISTS (
                   SELECT 1 FROM playback_positions WHERE course_id = ?1 AND resource_id = ?3)",
                params![course, old_resource, new_resource],
            )?;
        }
        tx.commit()?;
        Ok(outcome)
    }

    /// Whether a course file's text is indexed at this digest.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query failures.
    pub fn is_resource_indexed(
        &self,
        course_id: &str,
        resource_id: &str,
        sha256: &str,
    ) -> Result<bool, StorageError> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM indexed_resources WHERE course_id = ?1 AND resource_id = ?2 AND sha256 = ?3",
                [course_id, resource_id, sha256],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Replace the indexed passages taken from one course file.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for `SQLite` failures, including an unknown
    /// course.
    pub fn index_resource(
        &mut self,
        course_id: &str,
        resource_id: &str,
        sha256: &str,
        entries: &[SearchEntry],
    ) -> Result<(), StorageError> {
        let tx = self.connection.transaction()?;
        tx.execute(
            "DELETE FROM learning_search WHERE course_id = ?1 AND origin_id = ?2 AND kind IN ('transcript', 'text')",
            [course_id, resource_id],
        )?;
        tx.execute(
            "INSERT INTO indexed_resources (course_id, resource_id, sha256) VALUES (?1, ?2, ?3)
             ON CONFLICT(course_id, resource_id) DO UPDATE SET sha256 = excluded.sha256",
            [course_id, resource_id, sha256],
        )?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO learning_search (course_id, lesson_id, kind, source_id, origin_id, position, title, body)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', ?7)",
            )?;
            for entry in entries {
                insert.execute(params![
                    course_id,
                    entry.lesson_id,
                    entry.kind,
                    entry.source_id,
                    resource_id,
                    entry.position,
                    entry.body
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Best matches for an FTS5 query, titles weighted above text.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when `SQLite` rejects the query; callers
    /// should build it with care.
    pub fn search(&self, fts_query: &str, limit: usize) -> Result<Vec<SearchHit>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT s.course_id, s.lesson_id, s.kind, s.source_id, s.position,
                    snippet(learning_search, -1, char(57344), char(57345), '…', 16)
             FROM learning_search AS s JOIN courses AS c ON c.course_id = s.course_id
             WHERE learning_search MATCH ?1
             ORDER BY bm25(learning_search, 0, 0, 0, 0, 0, 0, 5.0, 1.0)
             LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![fts_query, sqlite_integer(limit as u64, "search limit")?],
            |row| {
                Ok(SearchHit {
                    course_id: row.get(0)?,
                    lesson_id: row.get(1)?,
                    kind: row.get(2)?,
                    source_id: row.get(3)?,
                    position: row.get(4)?,
                    snippet: row.get(5)?,
                })
            },
        )?;
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

    /// Move every stored course location under `from` to the same place
    /// under `to`, for when the app-data directory itself moves. Locations
    /// elsewhere, such as local folder imports, are left as they are.
    /// Returns how many locations changed.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for query, path-decoding, or write failures;
    /// nothing changes then.
    pub fn rebase_locations(&mut self, from: &Path, to: &Path) -> Result<usize, StorageError> {
        let tx = self.connection.transaction()?;
        let locations = {
            let mut statement =
                tx.prepare("SELECT course_id, path_encoding, path_bytes FROM course_locations")?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut changed = 0;
        for (course_id, encoding, bytes) in locations {
            let path = decode_path(&encoding, &bytes)?;
            let Ok(relative) = path.strip_prefix(from) else {
                continue;
            };
            let (path_encoding, path_bytes) = encode_path(&to.join(relative));
            tx.execute(
                "UPDATE course_locations SET path_encoding = ?2, path_bytes = ?3 WHERE course_id = ?1",
                params![course_id, path_encoding, path_bytes],
            )?;
            changed += 1;
        }
        tx.commit()?;
        Ok(changed)
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
        self.save_course(
            manifest,
            total_resource_bytes,
            package_root,
            imported_at,
            torrent,
            &CourseTrust::default(),
        )
    }

    fn save_course(
        &mut self,
        manifest: &CourseManifest,
        total_resource_bytes: u64,
        package_root: &Path,
        imported_at: u64,
        torrent: Option<(&str, &[u8])>,
        trust: &CourseTrust,
    ) -> Result<StoredCourseSummary, StorageError> {
        let total_bytes = sqlite_integer(total_resource_bytes, "total resource bytes")?;
        let imported_at_i64 = sqlite_integer(imported_at, "import timestamp")?;
        let manifest_json = serde_json::to_vec(manifest)?;
        let tx = self.connection.transaction()?;
        keep_previous_manifest(&tx, manifest, imported_at_i64)?;
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
        index_course_outline(&tx, manifest)?;
        record_trust(&tx, &manifest.course_id, trust, imported_at_i64)?;
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

/// Replace the course's recorded signature, and pin it when it has no pin.
fn record_trust(
    tx: &Transaction<'_>,
    course_id: &str,
    trust: &CourseTrust,
    now: i64,
) -> Result<(), StorageError> {
    tx.execute(
        "DELETE FROM course_signatures WHERE course_id = ?1",
        [course_id],
    )?;
    if let Some(signature) = &trust.signature {
        tx.execute(
            "INSERT INTO course_signatures (course_id, publisher_id, signing_key, signed_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                course_id,
                signature.publisher_id,
                signature.signing_key,
                sqlite_integer(signature.signed_at, "signature time")?,
            ],
        )?;
    }
    if let Some(publisher_id) = &trust.pin {
        tx.execute(
            "INSERT INTO course_pins (course_id, publisher_id, pinned_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(course_id) DO NOTHING",
            params![course_id, publisher_id, now],
        )?;
    }
    Ok(())
}

/// Before a course is replaced by another version, keep the manifest it had,
/// and drop history entries for the version arriving now and beyond the
/// newest [`KEPT_PREVIOUS_MANIFESTS`].
fn keep_previous_manifest(
    tx: &Transaction<'_>,
    manifest: &CourseManifest,
    replaced_at: i64,
) -> Result<(), StorageError> {
    let previous: Option<(String, Vec<u8>)> = tx
        .query_row(
            "SELECT course_version, manifest_json FROM courses WHERE course_id = ?1",
            [&manifest.course_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((version, json)) = previous.filter(|(version, _)| *version != manifest.version) {
        tx.execute(
            "INSERT INTO course_manifest_history (course_id, course_version, manifest_json, replaced_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(course_id, course_version) DO UPDATE SET
               manifest_json = excluded.manifest_json, replaced_at = excluded.replaced_at",
            params![manifest.course_id, version, json, replaced_at],
        )?;
    }
    tx.execute(
        "DELETE FROM course_manifest_history WHERE course_id = ?1 AND course_version = ?2",
        [&manifest.course_id, &manifest.version],
    )?;
    tx.execute(
        "DELETE FROM course_manifest_history WHERE course_id = ?1 AND rowid NOT IN (
           SELECT rowid FROM course_manifest_history WHERE course_id = ?1
           ORDER BY replaced_at DESC, rowid DESC LIMIT ?2)",
        params![manifest.course_id, KEPT_PREVIOUS_MANIFESTS],
    )?;
    Ok(())
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
    for (from, step) in TABLE_MIGRATIONS {
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

/// Schema 9. Learner notes, each anchored to a lesson or a place in one of
/// its resources. Updates are compare-and-swap on `revision`.
fn add_notes(connection: &mut Connection) -> Result<(), StorageError> {
    let tx = connection.transaction()?;
    tx.execute_batch(
        "CREATE TABLE notes (
           note_id TEXT PRIMARY KEY NOT NULL,
           course_id TEXT NOT NULL REFERENCES courses(course_id) ON DELETE CASCADE,
           lesson_id TEXT NOT NULL,
           anchor_json TEXT NOT NULL,
           body_json TEXT NOT NULL,
           body_text TEXT NOT NULL,
           revision INTEGER NOT NULL CHECK(revision >= 0),
           created_at INTEGER NOT NULL CHECK(created_at >= 0),
           updated_at INTEGER NOT NULL CHECK(updated_at >= 0)
         ) STRICT;
         CREATE INDEX notes_by_lesson ON notes (course_id, lesson_id, created_at);
         PRAGMA user_version = 9;",
    )?;
    tx.commit()?;
    Ok(())
}

/// Schema 10. One Excalidraw drawing per lesson, saved compare-and-swap on
/// `revision`, and the images placed in it, which never change once stored.
fn add_drawings(connection: &mut Connection) -> Result<(), StorageError> {
    let tx = connection.transaction()?;
    tx.execute_batch(
        "CREATE TABLE lesson_drawings (
           course_id TEXT NOT NULL REFERENCES courses(course_id) ON DELETE CASCADE,
           lesson_id TEXT NOT NULL,
           scene_json TEXT NOT NULL,
           revision INTEGER NOT NULL CHECK(revision >= 0),
           updated_at INTEGER NOT NULL CHECK(updated_at >= 0),
           PRIMARY KEY(course_id, lesson_id)
         ) STRICT;
         CREATE TABLE drawing_files (
           course_id TEXT NOT NULL,
           lesson_id TEXT NOT NULL,
           file_id TEXT NOT NULL,
           mime_type TEXT NOT NULL,
           data_url TEXT NOT NULL,
           created_at INTEGER NOT NULL CHECK(created_at >= 0),
           PRIMARY KEY(course_id, lesson_id, file_id),
           FOREIGN KEY(course_id, lesson_id) REFERENCES lesson_drawings(course_id, lesson_id) ON DELETE CASCADE
         ) STRICT;
         PRAGMA user_version = 10;",
    )?;
    tx.commit()?;
    Ok(())
}

/// Migrations from schema 4 on, by the version they start from.
const TABLE_MIGRATIONS: [(i64, Migration); 10] = [
    (4, add_playback_positions),
    (5, add_download_commitments),
    (6, add_pause_flag),
    (7, add_lesson_progress),
    (8, add_notes),
    (9, add_drawings),
    (10, add_learning_search),
    (11, add_manifest_history),
    (12, add_course_trust),
    (13, add_catalogs),
];

/// Schema 14. Publisher catalogs the learner imported (ADR 0014 decision 8).
fn add_catalogs(connection: &mut Connection) -> Result<(), StorageError> {
    let tx = connection.transaction()?;
    tx.execute_batch(
        "CREATE TABLE catalogs (
           publisher_id TEXT PRIMARY KEY NOT NULL,
           title TEXT NOT NULL,
           issued_at INTEGER NOT NULL CHECK(issued_at >= 0),
           source_url TEXT,
           document BLOB NOT NULL,
           imported_at INTEGER NOT NULL CHECK(imported_at >= 0)
         ) STRICT;
         PRAGMA user_version = 14;",
    )?;
    tx.commit()?;
    Ok(())
}

/// Schema 13. The verified signature of each course's current version, the
/// publisher each course is pinned to after its first signed import, and the
/// key revocations learned from catalogs (ADR 0014).
fn add_course_trust(connection: &mut Connection) -> Result<(), StorageError> {
    let tx = connection.transaction()?;
    tx.execute_batch(
        "CREATE TABLE course_signatures (
           course_id TEXT PRIMARY KEY NOT NULL REFERENCES courses(course_id) ON DELETE CASCADE,
           publisher_id TEXT NOT NULL,
           signing_key TEXT NOT NULL,
           signed_at INTEGER NOT NULL CHECK(signed_at >= 0)
         ) STRICT;
         CREATE TABLE course_pins (
           course_id TEXT PRIMARY KEY NOT NULL REFERENCES courses(course_id) ON DELETE CASCADE,
           publisher_id TEXT NOT NULL,
           pinned_at INTEGER NOT NULL CHECK(pinned_at >= 0)
         ) STRICT;
         CREATE TABLE key_revocations (
           statement_json TEXT PRIMARY KEY NOT NULL,
           recorded_at INTEGER NOT NULL CHECK(recorded_at >= 0)
         ) STRICT;
         PRAGMA user_version = 13;",
    )?;
    tx.commit()?;
    Ok(())
}

/// Schema 12. The manifests of the versions a course replaced, so a
/// learner's annotations on lessons a new version no longer has can still be
/// matched to its lessons, and a trigger that keeps moved notes searchable
/// under their new lesson.
fn add_manifest_history(connection: &mut Connection) -> Result<(), StorageError> {
    let tx = connection.transaction()?;
    tx.execute_batch(
        "CREATE TABLE course_manifest_history (
           course_id TEXT NOT NULL REFERENCES courses(course_id) ON DELETE CASCADE,
           course_version TEXT NOT NULL,
           manifest_json BLOB NOT NULL,
           replaced_at INTEGER NOT NULL CHECK(replaced_at >= 0),
           PRIMARY KEY(course_id, course_version)
         ) STRICT;
         CREATE TRIGGER notes_search_move AFTER UPDATE OF lesson_id ON notes BEGIN
           DELETE FROM learning_search WHERE kind = 'note' AND source_id = old.note_id;
           INSERT INTO learning_search (course_id, lesson_id, kind, source_id, origin_id, position, title, body)
           VALUES (new.course_id, new.lesson_id, 'note', new.note_id, new.note_id, NULL, '', new.body_text);
         END;
         PRAGMA user_version = 12;",
    )?;
    tx.commit()?;
    Ok(())
}

/// Schema 11. One full-text index over what a learner can search: course and
/// lesson titles, notes (kept in step by triggers), and the text of
/// transcripts and lesson documents, with the files it was taken from.
fn add_learning_search(connection: &mut Connection) -> Result<(), StorageError> {
    let tx = connection.transaction()?;
    tx.execute_batch(
        "CREATE VIRTUAL TABLE learning_search USING fts5(
           course_id UNINDEXED,
           lesson_id UNINDEXED,
           kind UNINDEXED,
           source_id UNINDEXED,
           origin_id UNINDEXED,
           position UNINDEXED,
           title,
           body,
           tokenize = 'unicode61 remove_diacritics 2'
         );
         CREATE TABLE indexed_resources (
           course_id TEXT NOT NULL REFERENCES courses(course_id) ON DELETE CASCADE,
           resource_id TEXT NOT NULL,
           sha256 TEXT NOT NULL,
           PRIMARY KEY(course_id, resource_id)
         ) STRICT;
         CREATE TRIGGER notes_search_insert AFTER INSERT ON notes BEGIN
           INSERT INTO learning_search (course_id, lesson_id, kind, source_id, origin_id, position, title, body)
           VALUES (new.course_id, new.lesson_id, 'note', new.note_id, new.note_id, NULL, '', new.body_text);
         END;
         CREATE TRIGGER notes_search_update AFTER UPDATE OF body_text ON notes BEGIN
           DELETE FROM learning_search WHERE kind = 'note' AND source_id = old.note_id;
           INSERT INTO learning_search (course_id, lesson_id, kind, source_id, origin_id, position, title, body)
           VALUES (new.course_id, new.lesson_id, 'note', new.note_id, new.note_id, NULL, '', new.body_text);
         END;
         CREATE TRIGGER notes_search_delete AFTER DELETE ON notes BEGIN
           DELETE FROM learning_search WHERE kind = 'note' AND source_id = old.note_id;
         END;
         INSERT INTO learning_search (course_id, lesson_id, kind, source_id, origin_id, position, title, body)
           SELECT course_id, lesson_id, 'note', note_id, note_id, NULL, '', body_text FROM notes;",
    )?;
    let manifests = {
        let mut statement = tx.prepare("SELECT manifest_json FROM courses")?;
        let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for manifest in manifests {
        let manifest: CourseManifest = serde_json::from_slice(&manifest)?;
        index_course_outline(&tx, &manifest)?;
    }
    tx.execute_batch("PRAGMA user_version = 11;")?;
    tx.commit()?;
    Ok(())
}

/// Replace a course's title and lesson rows in the learning index, and drop
/// text taken from its files: a new version may have changed them.
fn index_course_outline(
    tx: &Transaction<'_>,
    manifest: &CourseManifest,
) -> Result<(), StorageError> {
    tx.execute(
        "DELETE FROM learning_search WHERE course_id = ?1 AND kind <> 'note'",
        [&manifest.course_id],
    )?;
    tx.execute(
        "DELETE FROM indexed_resources WHERE course_id = ?1",
        [&manifest.course_id],
    )?;
    let modules: Vec<&str> = manifest
        .modules
        .iter()
        .map(|module| module.title.as_str())
        .collect();
    let course_body = [
        manifest.description.as_deref().unwrap_or_default(),
        &manifest.publisher.name,
        &manifest.tags.join(" "),
        &modules.join("\n"),
    ]
    .join("\n");
    let mut insert = tx.prepare(
        "INSERT INTO learning_search (course_id, lesson_id, kind, source_id, origin_id, position, title, body)
         VALUES (?1, ?2, ?3, ?4, ?4, NULL, ?5, ?6)",
    )?;
    insert.execute(params![
        manifest.course_id,
        "",
        "course",
        manifest.course_id,
        manifest.title,
        course_body
    ])?;
    for module in &manifest.modules {
        for lesson in &module.lessons {
            insert.execute(params![
                manifest.course_id,
                lesson.id,
                "lesson",
                lesson.id,
                lesson.title,
                module.title
            ])?;
        }
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
        torrent_info_hash: row.get(7)?,
        sharing: row.get(8)?,
    })
}

fn unix_now_u64() -> Result<u64, StorageError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| StorageError::InvalidSystemClock)
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
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
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
                    "DROP TABLE catalogs;
                     DROP TABLE key_revocations; DROP TABLE course_pins; DROP TABLE course_signatures;
                     DROP TABLE course_manifest_history;
                     DROP TABLE learning_search; DROP TABLE indexed_resources;
                     DROP TABLE drawing_files; DROP TABLE lesson_drawings;
                     DROP TABLE notes; DROP TABLE lesson_progress;
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
    fn version_ten_databases_index_existing_courses_and_notes() {
        let path =
            std::env::temp_dir().join(format!("zeron-learning-v10-{}.sqlite3", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let course = manifest("Distributed caching", "systems");
        {
            let mut storage = Storage::open(&path).unwrap();
            storage
                .save_imported_course(&course, 10, Path::new("/courses/cache"))
                .unwrap();
            storage
                .connection
                .execute_batch(
                    "DROP TABLE catalogs;
                     DROP TABLE key_revocations; DROP TABLE course_pins; DROP TABLE course_signatures;
                     DROP TABLE course_manifest_history; DROP TRIGGER notes_search_move;
                     DROP TRIGGER notes_search_insert; DROP TRIGGER notes_search_update;
                     DROP TRIGGER notes_search_delete;
                     DROP TABLE learning_search; DROP TABLE indexed_resources;
                     PRAGMA user_version = 10;",
                )
                .unwrap();
            storage
                .insert_note(
                    "note_1",
                    &course.course_id,
                    &course.modules[0].lessons[0].id,
                    r#"{"kind":"lesson"}"#,
                    "{}",
                    "Consistent hashing ring",
                )
                .unwrap();
        }
        let storage = Storage::open(&path).unwrap();
        let kinds = |query: &str| -> Vec<String> {
            storage
                .search(query, 5)
                .unwrap()
                .into_iter()
                .map(|hit| hit.kind)
                .collect()
        };
        assert_eq!(kinds("\"hashing\""), ["note"]);
        assert_eq!(kinds("\"distributed\""), ["course"]);
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
    fn notes_are_saved_only_from_their_current_revision() {
        let mut storage = Storage::open_in_memory().unwrap();
        let course = manifest("Rust", "systems");
        let id = course.course_id.clone();
        assert!(
            storage
                .insert_note("n1", &id, "l1", "{}", "{}", "")
                .is_err()
        );
        storage
            .save_imported_course(&course, 10, Path::new("/courses/rust"))
            .unwrap();
        let note = storage
            .insert_note("n1", &id, "l1", "a0", "b0", "t0")
            .unwrap();
        assert_eq!(note.revision, 0);
        assert!(storage.insert_note("n1", &id, "l1", "a", "b", "t").is_err());
        storage.insert_note("n2", &id, "l2", "a", "b", "t").unwrap();

        let NoteWrite::Saved(saved) = storage.update_note("n1", 0, "a1", "b1", "t1").unwrap()
        else {
            panic!("saved");
        };
        assert_eq!((saved.revision, saved.body_json.as_str()), (1, "b1"));
        assert_eq!(
            storage
                .update_note("n1", 0, "lost", "lost", "lost")
                .unwrap(),
            NoteWrite::Conflict {
                current_revision: 1
            }
        );
        assert_eq!(storage.note("n1").unwrap().unwrap().body_json, "b1");
        assert_eq!(
            storage.update_note("missing", 0, "a", "b", "t").unwrap(),
            NoteWrite::Missing
        );
        assert_eq!(
            storage.delete_note("n1", 0).unwrap(),
            NoteWrite::Conflict {
                current_revision: 1
            }
        );
        assert_eq!(storage.notes(&id, Some("l1")).unwrap().len(), 1);
        assert_eq!(storage.notes(&id, None).unwrap().len(), 2);
        assert!(matches!(
            storage.delete_note("n1", 1).unwrap(),
            NoteWrite::Saved(_)
        ));
        assert_eq!(storage.delete_note("n1", 1).unwrap(), NoteWrite::Missing);
        storage
            .save_imported_course(&course, 10, Path::new("/courses/rust"))
            .unwrap();
        assert_eq!(storage.notes(&id, None).unwrap().len(), 1);
    }

    #[test]
    fn drawings_are_saved_compare_and_swap_with_immutable_images() {
        let mut storage = Storage::open_in_memory().unwrap();
        let course = manifest("Rust", "systems");
        let id = course.course_id.clone();
        assert!(storage.save_drawing(&id, "l1", None, "{}", &[]).is_err());
        storage
            .save_drawing(&id, "l1", None, "{}", &[])
            .unwrap_err();
        storage
            .save_imported_course(&course, 10, Path::new("/courses/rust"))
            .unwrap();
        assert_eq!(storage.drawing(&id, "l1").unwrap(), None);
        let image = |content: &str| {
            (
                "img1".to_owned(),
                "image/png".to_owned(),
                content.to_owned(),
            )
        };
        assert_eq!(
            storage
                .save_drawing(&id, "l1", None, "s0", &[image("first")])
                .unwrap(),
            DrawingWrite::Saved { revision: 0 }
        );
        assert_eq!(
            storage.save_drawing(&id, "l1", None, "again", &[]).unwrap(),
            DrawingWrite::Conflict {
                current_revision: 0
            }
        );
        assert_eq!(
            storage
                .save_drawing(&id, "l1", Some(0), "s1", &[image("replaced?")])
                .unwrap(),
            DrawingWrite::Saved { revision: 1 }
        );
        assert_eq!(
            storage
                .save_drawing(&id, "l1", Some(0), "stale", &[])
                .unwrap(),
            DrawingWrite::Conflict {
                current_revision: 1
            }
        );
        let stored = storage.drawing(&id, "l1").unwrap().unwrap();
        assert_eq!((stored.scene_json.as_str(), stored.revision), ("s1", 1));
        assert_eq!(stored.files, [image("first")]);
        assert_eq!(
            storage.save_drawing(&id, "l2", Some(3), "s", &[]).unwrap(),
            DrawingWrite::Conflict {
                current_revision: 0
            }
        );
        assert_eq!(storage.drawing(&id, "l2").unwrap(), None);
    }

    #[test]
    fn searches_outlines_notes_and_indexed_text() {
        let mut storage = Storage::open_in_memory().unwrap();
        let course = manifest("Distributed caching", "systems");
        let id = course.course_id.clone();
        storage
            .save_imported_course(&course, 10, Path::new("/courses/cache"))
            .unwrap();
        let lesson = course.modules[0].lessons[0].id.clone();
        let hits = |storage: &Storage, query: &str| -> Vec<(String, String)> {
            storage
                .search(query, 20)
                .unwrap()
                .into_iter()
                .map(|hit| (hit.kind, hit.snippet))
                .collect()
        };
        assert_eq!(hits(&storage, "distributed")[0].0, "course");
        assert_eq!(
            hits(&storage, "distributed")[0].1,
            "\u{e000}Distributed\u{e001} caching"
        );

        storage
            .insert_note(
                "note_1",
                &id,
                &lesson,
                r#"{"kind":"lesson"}"#,
                "{}",
                "Consistent hashing ring",
            )
            .unwrap();
        assert_eq!(hits(&storage, "hashing").len(), 1);
        storage
            .update_note(
                "note_1",
                0,
                r#"{"kind":"lesson"}"#,
                "{}",
                "Rendezvous hashing",
            )
            .unwrap();
        assert_eq!(hits(&storage, "ring").len(), 0);
        assert_eq!(hits(&storage, "rendezvous")[0].0, "note");

        assert!(!storage.is_resource_indexed(&id, "subs", "abc").unwrap());
        let entry = |body: &str, position| SearchEntry {
            lesson_id: lesson.clone(),
            kind: "transcript",
            source_id: "video".to_owned(),
            position,
            body: body.to_owned(),
        };
        storage
            .index_resource(
                &id,
                "subs",
                "abc",
                &[
                    entry("write through caches", Some(30.0)),
                    entry("eviction policy", Some(60.0)),
                ],
            )
            .unwrap();
        assert!(storage.is_resource_indexed(&id, "subs", "abc").unwrap());
        assert!(!storage.is_resource_indexed(&id, "subs", "def").unwrap());
        let found = storage.search("eviction", 5).unwrap();
        assert_eq!(
            (found[0].source_id.as_str(), found[0].position),
            ("video", Some(60.0))
        );
        storage
            .index_resource(
                &id,
                "subs",
                "def",
                &[entry("least recently used", Some(5.0))],
            )
            .unwrap();
        assert_eq!(hits(&storage, "eviction").len(), 0);
        assert_eq!(hits(&storage, "recently").len(), 1);

        // A new version of the course drops text taken from its old files.
        storage
            .save_imported_course(&course, 10, Path::new("/courses/cache"))
            .unwrap();
        assert_eq!(hits(&storage, "recently").len(), 0);
        assert!(!storage.is_resource_indexed(&id, "subs", "def").unwrap());
        assert_eq!(hits(&storage, "rendezvous").len(), 1);
        storage.delete_note("note_1", 1).unwrap();
        assert_eq!(hits(&storage, "rendezvous").len(), 0);
        assert!(storage.search("\"unbalanced", 5).is_err());
    }

    #[test]
    fn locations_under_a_moved_directory_are_rebased() {
        let mut storage = Storage::open_in_memory().unwrap();
        let local = manifest("Local", "systems");
        let mut torrent = manifest("Torrent", "systems");
        torrent.course_id = "course_torrent".to_owned();
        storage
            .save_imported_course(&local, 10, Path::new("/home/me/courses/local"))
            .unwrap();
        storage
            .save_torrent_course(
                &torrent,
                10,
                Path::new("/data/app.zeron.learner/courses/abc"),
                &"a".repeat(40),
                b"metainfo",
            )
            .unwrap();
        let changed = storage
            .rebase_locations(
                Path::new("/data/app.zeron.learner"),
                Path::new("/data/app.zeron"),
            )
            .unwrap();
        assert_eq!(changed, 1);
        assert_eq!(
            storage.course_package_root(&torrent.course_id).unwrap(),
            Some(PathBuf::from("/data/app.zeron/courses/abc"))
        );
        assert_eq!(
            storage.course_package_root(&local.course_id).unwrap(),
            Some(PathBuf::from("/home/me/courses/local"))
        );
        // A sibling whose name merely starts the same is not under it.
        assert_eq!(
            storage
                .rebase_locations(Path::new("/data/app.zeron"), Path::new("/x"))
                .unwrap(),
            1
        );
        assert_eq!(
            storage
                .rebase_locations(Path::new("/data/app"), Path::new("/y"))
                .unwrap(),
            0
        );
    }

    #[test]
    fn replaced_versions_keep_their_manifests_newest_first_and_bounded() {
        let mut storage = Storage::open_in_memory().unwrap();
        let mut course = manifest("Rust", "systems");
        let id = course.course_id.clone();
        let root = Path::new("/courses/rust");
        course.version = "1.0.0".to_owned();
        storage
            .save_imported_course_at(&course, 1, root, 1, None)
            .unwrap();
        storage
            .save_imported_course_at(&course, 1, root, 2, None)
            .unwrap();
        assert_eq!(storage.previous_manifests(&id).unwrap().len(), 0);
        for (minor, at) in (1..=7).zip(3..) {
            course.version = format!("1.{minor}.0");
            storage
                .save_imported_course_at(&course, 1, root, at, None)
                .unwrap();
        }
        let versions: Vec<String> = storage
            .previous_manifests(&id)
            .unwrap()
            .into_iter()
            .map(|manifest| manifest.version)
            .collect();
        assert_eq!(versions, ["1.6.0", "1.5.0", "1.4.0", "1.3.0", "1.2.0"]);
        course.version = "1.4.0".to_owned();
        storage
            .save_imported_course_at(&course, 1, root, 20, None)
            .unwrap();
        let versions: Vec<String> = storage
            .previous_manifests(&id)
            .unwrap()
            .into_iter()
            .map(|manifest| manifest.version)
            .collect();
        assert_eq!(versions, ["1.7.0", "1.6.0", "1.5.0", "1.3.0", "1.2.0"]);
    }

    #[test]
    fn annotations_move_without_overwriting_the_target() {
        let mut storage = Storage::open_in_memory().unwrap();
        let course = manifest("Rust", "systems");
        let id = course.course_id.clone();
        storage
            .save_imported_course(&course, 10, Path::new("/courses/rust"))
            .unwrap();
        storage
            .insert_note(
                "n1",
                &id,
                "old",
                "a1",
                r#"{"type":"doc"}"#,
                "ownership rules",
            )
            .unwrap();
        storage
            .insert_note("n2", &id, "old", "a2", r#"{"type":"doc"}"#, "borrowing")
            .unwrap();
        storage
            .insert_note("n3", &id, "other", "a3", r#"{"type":"doc"}"#, "kept")
            .unwrap();
        storage
            .save_drawing(
                &id,
                "old",
                None,
                "scene-old",
                &[("f1".to_owned(), "image/png".to_owned(), "data:a".to_owned())],
            )
            .unwrap();
        storage.mark_lesson_started(&id, "old").unwrap();
        storage.set_lesson_completed(&id, "old", true).unwrap();
        let position = PlaybackPosition {
            position_ms: 5_000,
            duration_ms: 9_000,
        };
        storage
            .set_playback_position(&id, "video_old", position)
            .unwrap();

        let outcome = storage
            .move_lesson_annotations(&AnnotationMoveRequest {
                course_id: id.clone(),
                from_lesson_id: "old".to_owned(),
                to_lesson_id: "new".to_owned(),
                note_anchors: vec![
                    ("n1".to_owned(), "b1".to_owned()),
                    ("n2".to_owned(), "b2".to_owned()),
                    ("n3".to_owned(), "x".to_owned()),
                ],
                resource_map: vec![("video_old".to_owned(), "video_new".to_owned())],
            })
            .unwrap();
        assert_eq!(
            outcome,
            AnnotationMoveOutcome {
                notes_moved: 2,
                drawing_moved: true,
                drawing_kept: false,
                progress_moved: true,
                positions_moved: 1
            }
        );
        let moved = storage.notes(&id, Some("new")).unwrap();
        assert_eq!(
            moved
                .iter()
                .map(|note| note.anchor_json.as_str())
                .collect::<Vec<_>>(),
            ["b1", "b2"]
        );
        assert!(moved.iter().all(|note| note.revision == 1));
        assert_eq!(storage.note("n3").unwrap().unwrap().lesson_id, "other");
        let drawing = storage.drawing(&id, "new").unwrap().unwrap();
        assert_eq!(drawing.scene_json, "scene-old");
        assert_eq!(drawing.files.len(), 1);
        assert!(storage.drawing(&id, "old").unwrap().is_none());
        assert_eq!(storage.lesson_progress(&id).unwrap()[0].lesson_id, "new");
        assert!(storage.lesson_progress(&id).unwrap()[0].completed);
        assert_eq!(
            storage.playback_position(&id, "video_new").unwrap(),
            Some(position)
        );
        let hits = storage.search("\"ownership\"", 5).unwrap();
        assert_eq!(hits[0].lesson_id, "new");
    }

    #[test]
    fn a_move_onto_a_lesson_with_its_own_annotations_keeps_them() {
        let mut storage = Storage::open_in_memory().unwrap();
        let course = manifest("Rust", "systems");
        let id = course.course_id.clone();
        storage
            .save_imported_course(&course, 10, Path::new("/courses/rust"))
            .unwrap();
        let position = PlaybackPosition {
            position_ms: 5_000,
            duration_ms: 9_000,
        };
        storage
            .save_drawing(&id, "new", None, "scene-old", &[])
            .unwrap();
        storage.mark_lesson_started(&id, "new").unwrap();
        storage
            .set_playback_position(&id, "video_new", position)
            .unwrap();

        storage
            .save_drawing(&id, "elsewhere", None, "scene-target", &[])
            .unwrap();
        storage.mark_lesson_started(&id, "elsewhere").unwrap();
        storage
            .set_playback_position(&id, "video_elsewhere", position)
            .unwrap();
        let kept = storage
            .move_lesson_annotations(&AnnotationMoveRequest {
                course_id: id.clone(),
                from_lesson_id: "new".to_owned(),
                to_lesson_id: "elsewhere".to_owned(),
                note_anchors: Vec::new(),
                resource_map: vec![("video_new".to_owned(), "video_elsewhere".to_owned())],
            })
            .unwrap();
        assert_eq!(
            kept,
            AnnotationMoveOutcome {
                notes_moved: 0,
                drawing_moved: false,
                drawing_kept: true,
                progress_moved: false,
                positions_moved: 0
            }
        );
        assert_eq!(
            storage
                .drawing(&id, "elsewhere")
                .unwrap()
                .unwrap()
                .scene_json,
            "scene-target"
        );
        assert_eq!(
            storage.drawing(&id, "new").unwrap().unwrap().scene_json,
            "scene-old"
        );
        assert_eq!(storage.drawing_lessons(&id).unwrap(), ["elsewhere", "new"]);
    }

    #[test]
    fn signatures_follow_each_version_and_pins_are_set_once() {
        let mut storage = Storage::open_in_memory().unwrap();
        let course = manifest("Rust", "systems");
        let id = course.course_id.clone();
        let root = Path::new("/courses/rust");
        let signed = |key: &str| CourseTrust {
            signature: Some(CourseSignature {
                publisher_id: "ed25519-root".to_owned(),
                signing_key: key.to_owned(),
                signed_at: 7,
            }),
            pin: Some("ed25519-root".to_owned()),
        };
        storage
            .save_imported_course_trusted(&course, 1, root, &CourseTrust::default())
            .unwrap();
        assert_eq!(storage.course_pin(&id).unwrap(), None);
        assert_eq!(storage.course_signature(&id).unwrap(), None);
        storage
            .save_imported_course_trusted(&course, 1, root, &signed("ed25519-a"))
            .unwrap();
        assert_eq!(
            storage.course_pin(&id).unwrap().as_deref(),
            Some("ed25519-root")
        );
        assert_eq!(
            storage.course_signature(&id).unwrap().unwrap().signing_key,
            "ed25519-a"
        );
        let other_pin = CourseTrust {
            pin: Some("ed25519-other".to_owned()),
            ..signed("ed25519-b")
        };
        storage
            .save_torrent_course_trusted(&course, 1, root, ("aa", b"meta"), &other_pin)
            .unwrap();
        assert_eq!(
            storage.course_pin(&id).unwrap().as_deref(),
            Some("ed25519-root")
        );
        assert_eq!(
            storage.course_signature(&id).unwrap().unwrap().signing_key,
            "ed25519-b"
        );
        assert!(storage.forget_course_pin(&id).unwrap());
        assert!(!storage.forget_course_pin(&id).unwrap());
        assert_eq!(storage.course_pin(&id).unwrap(), None);
        storage.save_imported_course(&course, 1, root).unwrap();
        assert_eq!(storage.course_signature(&id).unwrap(), None);
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
