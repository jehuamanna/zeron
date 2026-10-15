//! `SQLite` persistence for mutable publisher drafts and immutable release records.

use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use course_publisher::DraftCourse;
use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

const SCHEMA_VERSION: i64 = 3;

/// Path-free draft information for Admin course lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftSummary {
    /// Stable draft identity.
    pub draft_id: String,
    /// Stable logical course identity.
    pub course_id: String,
    /// Current intended course version.
    pub course_version: String,
    /// Display title.
    pub title: String,
    /// Current optimistic-concurrency revision.
    pub revision: u64,
    /// Last successful save time as Unix seconds.
    pub updated_at_unix_seconds: u64,
}

/// Backend-only draft snapshot with its durable save time.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredDraft {
    /// Exact saved snapshot, including backend-only source references.
    pub draft: DraftCourse,
    /// Last successful save time as Unix seconds.
    pub updated_at_unix_seconds: u64,
}

/// Lifecycle of an activated release. Release bytes never change in any state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseStatus {
    /// The course's current release.
    Published,
    /// A newer release of the same course was published.
    Superseded,
    /// Retained but hidden from active management.
    Archived,
}

impl ReleaseStatus {
    /// Stable lowercase name, as stored and exposed over IPC.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Superseded => "superseded",
            Self::Archived => "archived",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "published" => Some(Self::Published),
            "superseded" => Some(Self::Superseded),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

/// Facts recorded when a verified preparation becomes an immutable release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRelease {
    /// Opaque release identity.
    pub release_id: String,
    /// Logical course identity.
    pub course_id: String,
    /// Released course version; unique per course.
    pub course_version: String,
    /// Display title at release time.
    pub title: String,
    /// Draft the release was prepared from.
    pub draft_id: String,
    /// Draft revision frozen into the release.
    pub draft_revision: u64,
    /// SHA-256 of the released `course.json`.
    pub manifest_sha256: String,
    /// v1 info hash.
    pub info_hash: String,
    /// Magnet link.
    pub magnet: String,
    /// Torrent piece length.
    pub piece_length: u64,
    /// Torrent piece count.
    pub piece_count: u64,
    /// Number of package files, including `course.json`.
    pub file_count: u64,
    /// Total package bytes.
    pub total_bytes: u64,
}

/// A durable, immutable release record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseRecord {
    /// Recorded release facts.
    pub release: NewRelease,
    /// Activation time as Unix seconds.
    pub published_at_unix_seconds: u64,
    /// Current lifecycle status.
    pub status: ReleaseStatus,
}

/// Publisher persistence failure.
#[derive(Debug, Error)]
pub enum PublisherStorageError {
    /// `SQLite` rejected an operation.
    #[error("publisher SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Draft serialization or recovery failed.
    #[error("publisher draft JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    /// Another editor already stored a different revision.
    #[error("draft revision conflict: expected {expected}, actual {actual}")]
    RevisionConflict {
        /// Revision supplied by the saving editor.
        expected: u64,
        /// Durable revision at rejection time.
        actual: u64,
    },
    /// The draft snapshot revision is not exactly the next revision.
    #[error("draft snapshot revision must advance from {expected} to {actual}")]
    InvalidRevisionAdvance {
        /// Required next revision.
        expected: u64,
        /// Revision in the supplied snapshot.
        actual: u64,
    },
    /// A numeric value cannot be represented safely by `SQLite`.
    #[error("publisher storage value is too large for SQLite: {0}")]
    IntegerOverflow(&'static str),
    /// The system clock cannot produce a Unix timestamp.
    #[error("system clock is before the Unix epoch")]
    InvalidSystemClock,
    /// The course version was already released; versions are never reused.
    #[error("course version {course_version} was already released")]
    VersionAlreadyReleased {
        /// Rejected version.
        course_version: String,
    },
    /// A stored release status is not recognized.
    #[error("stored release status `{0}` is not recognized")]
    UnknownReleaseStatus(String),
    /// The database was created by a newer application.
    #[error("publisher schema version {found} is newer than supported version {supported}")]
    FutureSchema {
        /// Version found in the file.
        found: i64,
        /// Maximum understood version.
        supported: i64,
    },
}

/// Open, migrated publisher draft database.
pub struct PublisherStorage {
    connection: Connection,
}

impl PublisherStorage {
    /// Open and migrate a file-backed publisher database.
    ///
    /// # Errors
    /// Returns an error when configuration or migration fails.
    pub fn open(path: &Path) -> Result<Self, PublisherStorageError> {
        Self::initialize(Connection::open(path)?)
    }

    /// Open an isolated publisher database for tests.
    ///
    /// # Errors
    /// Returns an error when configuration or migration fails.
    pub fn open_in_memory() -> Result<Self, PublisherStorageError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(mut connection: Connection) -> Result<Self, PublisherStorageError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&mut connection)?;
        Ok(Self { connection })
    }

    /// Insert a new draft at revision zero.
    ///
    /// # Errors
    /// Rejects nonzero initial revisions, duplicate IDs, clock, JSON, or `SQLite` failures.
    pub fn create_draft(
        &mut self,
        draft: &DraftCourse,
    ) -> Result<DraftSummary, PublisherStorageError> {
        if draft.revision != 0 {
            return Err(PublisherStorageError::InvalidRevisionAdvance {
                expected: 0,
                actual: draft.revision,
            });
        }
        let updated_at = now()?;
        let snapshot = serde_json::to_vec(draft)?;
        self.connection.execute(
            "INSERT INTO drafts (draft_id, course_id, course_version, title, revision, snapshot_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
            params![
                draft.draft_id,
                draft.metadata.course_id,
                draft.metadata.version,
                draft.metadata.title,
                snapshot,
                to_i64(updated_at, "update timestamp")?,
            ],
        )?;
        Ok(summary(draft, updated_at))
    }

    /// Save one editor mutation using compare-and-swap revision semantics.
    ///
    /// # Errors
    /// Rejects skipped revisions, stale writers, or persistence failures.
    pub fn update_draft(
        &mut self,
        expected_revision: u64,
        draft: &DraftCourse,
    ) -> Result<DraftSummary, PublisherStorageError> {
        let required_revision = expected_revision.saturating_add(1);
        if draft.revision != required_revision {
            return Err(PublisherStorageError::InvalidRevisionAdvance {
                expected: required_revision,
                actual: draft.revision,
            });
        }
        let updated_at = now()?;
        let snapshot = serde_json::to_vec(draft)?;
        let changed = self.connection.execute(
            "UPDATE drafts SET course_id=?1, course_version=?2, title=?3, revision=?4, snapshot_json=?5, updated_at=?6
             WHERE draft_id=?7 AND revision=?8",
            params![
                draft.metadata.course_id,
                draft.metadata.version,
                draft.metadata.title,
                to_i64(draft.revision, "draft revision")?,
                snapshot,
                to_i64(updated_at, "update timestamp")?,
                draft.draft_id,
                to_i64(expected_revision, "expected revision")?,
            ],
        )?;
        if changed == 0 {
            let actual = self.current_revision(&draft.draft_id)?.unwrap_or(0);
            return Err(PublisherStorageError::RevisionConflict {
                expected: expected_revision,
                actual,
            });
        }
        Ok(summary(draft, updated_at))
    }

    /// Load the exact saved draft snapshot.
    ///
    /// # Errors
    /// Returns an error for database or corrupt JSON failures.
    pub fn draft(&self, draft_id: &str) -> Result<Option<DraftCourse>, PublisherStorageError> {
        Ok(self.stored_draft(draft_id)?.map(|stored| stored.draft))
    }

    /// Load the exact saved draft snapshot with its last save time.
    ///
    /// # Errors
    /// Returns an error for database or corrupt JSON failures.
    pub fn stored_draft(
        &self,
        draft_id: &str,
    ) -> Result<Option<StoredDraft>, PublisherStorageError> {
        let row: Option<(Vec<u8>, i64)> = self
            .connection
            .query_row(
                "SELECT snapshot_json, updated_at FROM drafts WHERE draft_id=?1",
                [draft_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(bytes, updated_at)| {
            Ok(StoredDraft {
                draft: serde_json::from_slice(&bytes)?,
                updated_at_unix_seconds: from_i64(updated_at, 1)?,
            })
        })
        .transpose()
    }

    /// List path-free draft summaries in most-recently-saved order.
    ///
    /// # Errors
    /// Returns an error when a row cannot be queried or decoded.
    pub fn list_drafts(&self) -> Result<Vec<DraftSummary>, PublisherStorageError> {
        let mut statement = self.connection.prepare(
            "SELECT draft_id, course_id, course_version, title, revision, updated_at
             FROM drafts ORDER BY updated_at DESC, draft_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(DraftSummary {
                draft_id: row.get(0)?,
                course_id: row.get(1)?,
                course_version: row.get(2)?,
                title: row.get(3)?,
                revision: from_i64(row.get(4)?, 4)?,
                updated_at_unix_seconds: from_i64(row.get(5)?, 5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(PublisherStorageError::from)
    }

    /// Record a release, marking the course's previously published release superseded.
    ///
    /// # Errors
    /// Returns [`PublisherStorageError::VersionAlreadyReleased`] when the
    /// course version already has a release, or a storage failure.
    pub fn insert_release(
        &mut self,
        release: NewRelease,
    ) -> Result<ReleaseRecord, PublisherStorageError> {
        let published_at = now()?;
        let transaction = self.connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM releases WHERE course_id=?1 AND course_version=?2)",
            params![release.course_id, release.course_version],
            |row| row.get(0),
        )?;
        if exists {
            return Err(PublisherStorageError::VersionAlreadyReleased {
                course_version: release.course_version,
            });
        }
        transaction.execute(
            "UPDATE releases SET status='superseded' WHERE course_id=?1 AND status='published'",
            [&release.course_id],
        )?;
        transaction.execute(
            "INSERT INTO releases (release_id, course_id, course_version, title, draft_id, draft_revision,
                manifest_sha256, info_hash, magnet, piece_length, piece_count, file_count, total_bytes,
                published_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 'published')",
            params![
                release.release_id,
                release.course_id,
                release.course_version,
                release.title,
                release.draft_id,
                to_i64(release.draft_revision, "draft revision")?,
                release.manifest_sha256,
                release.info_hash,
                release.magnet,
                to_i64(release.piece_length, "piece length")?,
                to_i64(release.piece_count, "piece count")?,
                to_i64(release.file_count, "file count")?,
                to_i64(release.total_bytes, "total bytes")?,
                to_i64(published_at, "publish timestamp")?,
            ],
        )?;
        transaction.commit()?;
        Ok(ReleaseRecord {
            release,
            published_at_unix_seconds: published_at,
            status: ReleaseStatus::Published,
        })
    }

    /// Whether a course version already has a release.
    ///
    /// # Errors
    /// Returns an error when storage cannot be queried.
    pub fn version_released(
        &self,
        course_id: &str,
        course_version: &str,
    ) -> Result<bool, PublisherStorageError> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM releases WHERE course_id=?1 AND course_version=?2)",
            params![course_id, course_version],
            |row| row.get(0),
        )?)
    }

    /// Load one release record.
    ///
    /// # Errors
    /// Returns an error when storage cannot be queried or decoded.
    pub fn release(
        &self,
        release_id: &str,
    ) -> Result<Option<ReleaseRecord>, PublisherStorageError> {
        Ok(self
            .query_releases("WHERE release_id=?1", [release_id])?
            .into_iter()
            .next())
    }

    /// All releases, newest first.
    ///
    /// # Errors
    /// Returns an error when storage cannot be queried or decoded.
    pub fn releases(&self) -> Result<Vec<ReleaseRecord>, PublisherStorageError> {
        self.query_releases("", [])
    }

    fn query_releases(
        &self,
        filter: &str,
        parameters: impl rusqlite::Params,
    ) -> Result<Vec<ReleaseRecord>, PublisherStorageError> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT release_id, course_id, course_version, title, draft_id, draft_revision,
                manifest_sha256, info_hash, magnet, piece_length, piece_count, file_count, total_bytes,
                published_at, status
             FROM releases {filter} ORDER BY published_at DESC, release_id"
        ))?;
        let rows = statement.query_map(parameters, |row| {
            Ok((
                NewRelease {
                    release_id: row.get(0)?,
                    course_id: row.get(1)?,
                    course_version: row.get(2)?,
                    title: row.get(3)?,
                    draft_id: row.get(4)?,
                    draft_revision: from_i64(row.get(5)?, 5)?,
                    manifest_sha256: row.get(6)?,
                    info_hash: row.get(7)?,
                    magnet: row.get(8)?,
                    piece_length: from_i64(row.get(9)?, 9)?,
                    piece_count: from_i64(row.get(10)?, 10)?,
                    file_count: from_i64(row.get(11)?, 11)?,
                    total_bytes: from_i64(row.get(12)?, 12)?,
                },
                from_i64(row.get(13)?, 13)?,
                row.get::<_, String>(14)?,
            ))
        })?;
        rows.map(|row| {
            let (release, published_at, status) = row?;
            Ok(ReleaseRecord {
                release,
                published_at_unix_seconds: published_at,
                status: ReleaseStatus::parse(&status)
                    .ok_or(PublisherStorageError::UnknownReleaseStatus(status))?,
            })
        })
        .collect()
    }

    /// Persist whether a release should be seeded whenever the Admin app runs.
    ///
    /// # Errors
    /// Returns a storage failure, including for unknown releases.
    pub fn set_seeding_intent(
        &mut self,
        release_id: &str,
        enabled: bool,
    ) -> Result<(), PublisherStorageError> {
        if enabled {
            self.connection.execute(
                "INSERT INTO seeding_intents (release_id, updated_at) VALUES (?1, ?2)
                 ON CONFLICT (release_id) DO UPDATE SET updated_at = excluded.updated_at",
                params![release_id, to_i64(now()?, "update timestamp")?],
            )?;
        } else {
            self.connection.execute(
                "DELETE FROM seeding_intents WHERE release_id=?1",
                [release_id],
            )?;
        }
        Ok(())
    }

    /// Releases the publisher wants seeded, oldest intent first.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn seeding_intents(&self) -> Result<Vec<String>, PublisherStorageError> {
        let mut statement = self
            .connection
            .prepare("SELECT release_id FROM seeding_intents ORDER BY updated_at, release_id")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(PublisherStorageError::from)
    }

    /// Read an application setting.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn setting(&self, key: &str) -> Result<Option<String>, PublisherStorageError> {
        Ok(self
            .connection
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Write an application setting.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), PublisherStorageError> {
        self.connection.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    fn current_revision(&self, draft_id: &str) -> Result<Option<u64>, PublisherStorageError> {
        let value: Option<i64> = self
            .connection
            .query_row(
                "SELECT revision FROM drafts WHERE draft_id=?1",
                [draft_id],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|revision| {
                u64::try_from(revision)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, revision))
            })
            .transpose()
            .map_err(PublisherStorageError::from)
    }
}

fn migrate(connection: &mut Connection) -> Result<(), PublisherStorageError> {
    let found: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if found > SCHEMA_VERSION {
        return Err(PublisherStorageError::FutureSchema {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    if found < 1 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
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
        )?;
        transaction.commit()?;
    }
    if found < 2 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE releases (
                release_id TEXT PRIMARY KEY NOT NULL,
                course_id TEXT NOT NULL,
                course_version TEXT NOT NULL,
                title TEXT NOT NULL,
                draft_id TEXT NOT NULL,
                draft_revision INTEGER NOT NULL CHECK (draft_revision >= 0),
                manifest_sha256 TEXT NOT NULL,
                info_hash TEXT NOT NULL,
                magnet TEXT NOT NULL,
                piece_length INTEGER NOT NULL CHECK (piece_length > 0),
                piece_count INTEGER NOT NULL CHECK (piece_count >= 0),
                file_count INTEGER NOT NULL CHECK (file_count > 0),
                total_bytes INTEGER NOT NULL CHECK (total_bytes >= 0),
                published_at INTEGER NOT NULL CHECK (published_at >= 0),
                status TEXT NOT NULL CHECK (status IN ('published', 'superseded', 'archived')),
                UNIQUE (course_id, course_version)
             );
             CREATE INDEX releases_by_course ON releases (course_id, published_at);
             PRAGMA user_version = 2;",
        )?;
        transaction.commit()?;
    }
    if found < 3 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE seeding_intents (
                release_id TEXT PRIMARY KEY NOT NULL REFERENCES releases (release_id),
                updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
             );
             CREATE TABLE settings (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
             );
             PRAGMA user_version = 3;",
        )?;
        transaction.commit()?;
    }
    Ok(())
}

fn summary(draft: &DraftCourse, updated_at: u64) -> DraftSummary {
    DraftSummary {
        draft_id: draft.draft_id.clone(),
        course_id: draft.metadata.course_id.clone(),
        course_version: draft.metadata.version.clone(),
        title: draft.metadata.title.clone(),
        revision: draft.revision,
        updated_at_unix_seconds: updated_at,
    }
}

fn now() -> Result<u64, PublisherStorageError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| PublisherStorageError::InvalidSystemClock)
}

fn to_i64(value: u64, field: &'static str) -> Result<i64, PublisherStorageError> {
    i64::try_from(value).map_err(|_| PublisherStorageError::IntegerOverflow(field))
}

fn from_i64(value: i64, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}
