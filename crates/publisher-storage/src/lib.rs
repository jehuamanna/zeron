//! `SQLite` persistence for mutable publisher drafts.

use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use course_publisher::DraftCourse;
use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

const SCHEMA_VERSION: i64 = 1;

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
        let snapshot: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT snapshot_json FROM drafts WHERE draft_id=?1",
                [draft_id],
                |row| row.get(0),
            )
            .optional()?;
        snapshot
            .map(|bytes| serde_json::from_slice(&bytes))
            .transpose()
            .map_err(PublisherStorageError::from)
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
    if found == 0 {
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
