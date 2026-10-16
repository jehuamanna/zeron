//! The issuer's SQLite database (ADR 0017 decision 6). It stores only what
//! issuing needs: each publisher's delegation and chain, which publisher owns
//! each course, registered releases with their content keys sealed to the
//! issuer, entitlements by learner key, and a log of issued licenses. No
//! names, email addresses, or payment details.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::IssuerError;

/// The database file's name in the data directory.
pub const DATABASE_FILE: &str = "issuer.sqlite3";
const SCHEMA_VERSION: i64 = 1;

/// A registered release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredRelease {
    /// The course version.
    pub course_version: String,
    /// Its content key sealed to the issuer.
    pub sealed_content_key: String,
}

/// What registering a release changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    /// A new release was recorded.
    Added,
    /// The same release was already recorded.
    Unchanged,
}

/// The issuer database.
pub struct Storage {
    connection: Connection,
}

impl Storage {
    /// Open or create the database in `directory`.
    ///
    /// # Errors
    /// Returns [`IssuerError::Storage`].
    pub fn open(directory: &Path) -> Result<Self, IssuerError> {
        Self::migrate(Connection::open(directory.join(DATABASE_FILE))?)
    }

    /// An in-memory database, for tests.
    ///
    /// # Errors
    /// Returns [`IssuerError::Storage`].
    pub fn open_in_memory() -> Result<Self, IssuerError> {
        Self::migrate(Connection::open_in_memory()?)
    }

    fn migrate(mut connection: Connection) -> Result<Self, IssuerError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let found: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if found > SCHEMA_VERSION {
            return Err(IssuerError::Storage(format!(
                "the database is schema {found}; this issuer understands {SCHEMA_VERSION}"
            )));
        }
        if found < 1 {
            let tx = connection.transaction()?;
            tx.execute_batch(
                "CREATE TABLE publishers (
                   publisher_id TEXT PRIMARY KEY NOT NULL,
                   delegation_json TEXT NOT NULL,
                   rotations_json TEXT NOT NULL,
                   updated_at INTEGER NOT NULL CHECK(updated_at >= 0)
                 ) STRICT;
                 CREATE TABLE courses (
                   course_id TEXT PRIMARY KEY NOT NULL,
                   publisher_id TEXT NOT NULL REFERENCES publishers(publisher_id)
                 ) STRICT;
                 CREATE TABLE releases (
                   course_id TEXT NOT NULL REFERENCES courses(course_id),
                   course_version TEXT NOT NULL,
                   sealed_content_key TEXT NOT NULL,
                   registered_at INTEGER NOT NULL CHECK(registered_at >= 0),
                   PRIMARY KEY(course_id, course_version)
                 ) STRICT;
                 CREATE TABLE entitlements (
                   course_id TEXT NOT NULL,
                   learner_key TEXT NOT NULL,
                   granted_at INTEGER NOT NULL CHECK(granted_at >= 0),
                   source TEXT NOT NULL,
                   PRIMARY KEY(course_id, learner_key)
                 ) STRICT;
                 CREATE TABLE issued_licenses (
                   course_id TEXT NOT NULL,
                   learner_key TEXT NOT NULL,
                   issued_at INTEGER NOT NULL CHECK(issued_at >= 0),
                   versions INTEGER NOT NULL CHECK(versions > 0)
                 ) STRICT;
                 PRAGMA user_version = 1;",
            )?;
            tx.commit()?;
        }
        Ok(Self { connection })
    }

    /// Record or replace a publisher's delegation and chain.
    ///
    /// # Errors
    /// Returns [`IssuerError::Storage`].
    pub fn set_publisher(
        &mut self,
        publisher_id: &str,
        delegation_json: &str,
        rotations_json: &str,
        now: u64,
    ) -> Result<(), IssuerError> {
        self.connection.execute(
            "INSERT INTO publishers (publisher_id, delegation_json, rotations_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(publisher_id) DO UPDATE SET
               delegation_json = excluded.delegation_json,
               rotations_json = excluded.rotations_json,
               updated_at = excluded.updated_at",
            params![publisher_id, delegation_json, rotations_json, seconds(now)],
        )?;
        Ok(())
    }

    /// Keep a longer rotation chain for a publisher, from a later request.
    ///
    /// # Errors
    /// Returns [`IssuerError::Storage`].
    pub fn set_rotations(
        &mut self,
        publisher_id: &str,
        rotations_json: &str,
    ) -> Result<(), IssuerError> {
        self.connection.execute(
            "UPDATE publishers SET rotations_json = ?2 WHERE publisher_id = ?1",
            params![publisher_id, rotations_json],
        )?;
        Ok(())
    }

    /// A publisher's delegation and rotations, as JSON.
    ///
    /// # Errors
    /// Returns [`IssuerError::Storage`].
    pub fn publisher(&self, publisher_id: &str) -> Result<Option<(String, String)>, IssuerError> {
        Ok(self
            .connection
            .query_row(
                "SELECT delegation_json, rotations_json FROM publishers WHERE publisher_id = ?1",
                [publisher_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    /// The publisher that owns a course on this issuer.
    ///
    /// # Errors
    /// Returns [`IssuerError::Storage`].
    pub fn course_publisher(&self, course_id: &str) -> Result<Option<String>, IssuerError> {
        Ok(self
            .connection
            .query_row(
                "SELECT publisher_id FROM courses WHERE course_id = ?1",
                [course_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Record a release. The first registration of a course makes its
    /// publisher the course's owner here. A version, once registered, keeps
    /// its content key.
    ///
    /// # Errors
    /// Returns [`IssuerError::CourseOwned`] for a course another publisher
    /// owns, [`IssuerError::VersionRegistered`] for a version registered with
    /// another key, or [`IssuerError::Storage`].
    pub fn register_release(
        &mut self,
        publisher_id: &str,
        course_id: &str,
        course_version: &str,
        sealed_content_key: &str,
        now: u64,
    ) -> Result<Registration, IssuerError> {
        let tx = self.connection.transaction()?;
        let owner: Option<String> = tx
            .query_row(
                "SELECT publisher_id FROM courses WHERE course_id = ?1",
                [course_id],
                |row| row.get(0),
            )
            .optional()?;
        match owner {
            Some(owner) if owner != publisher_id => return Err(IssuerError::CourseOwned),
            Some(_) => {}
            None => {
                tx.execute(
                    "INSERT INTO courses (course_id, publisher_id) VALUES (?1, ?2)",
                    params![course_id, publisher_id],
                )?;
            }
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT sealed_content_key FROM releases WHERE course_id = ?1 AND course_version = ?2",
                params![course_id, course_version],
                |row| row.get(0),
            )
            .optional()?;
        let outcome = match existing {
            Some(existing) if existing == sealed_content_key => Registration::Unchanged,
            Some(_) => return Err(IssuerError::VersionRegistered),
            None => {
                tx.execute(
                    "INSERT INTO releases (course_id, course_version, sealed_content_key, registered_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![course_id, course_version, sealed_content_key, seconds(now)],
                )?;
                Registration::Added
            }
        };
        tx.commit()?;
        Ok(outcome)
    }

    /// A course's registered releases, oldest first.
    ///
    /// # Errors
    /// Returns [`IssuerError::Storage`].
    pub fn releases(&self, course_id: &str) -> Result<Vec<RegisteredRelease>, IssuerError> {
        let mut statement = self.connection.prepare(
            "SELECT course_version, sealed_content_key FROM releases
             WHERE course_id = ?1 ORDER BY registered_at, rowid",
        )?;
        let rows = statement.query_map([course_id], |row| {
            Ok(RegisteredRelease {
                course_version: row.get(0)?,
                sealed_content_key: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Entitle a learner key to a course. Granting again keeps the first
    /// grant.
    ///
    /// # Errors
    /// Returns [`IssuerError::Storage`].
    pub fn grant(
        &mut self,
        course_id: &str,
        learner_key: &str,
        source: &str,
        now: u64,
    ) -> Result<bool, IssuerError> {
        let inserted = self.connection.execute(
            "INSERT INTO entitlements (course_id, learner_key, granted_at, source) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(course_id, learner_key) DO NOTHING",
            params![course_id, learner_key, seconds(now), source],
        )?;
        Ok(inserted == 1)
    }

    /// Whether a learner key is entitled to a course.
    ///
    /// # Errors
    /// Returns [`IssuerError::Storage`].
    pub fn entitled(&self, course_id: &str, learner_key: &str) -> Result<bool, IssuerError> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM entitlements WHERE course_id = ?1 AND learner_key = ?2",
                params![course_id, learner_key],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Log an issued license.
    ///
    /// # Errors
    /// Returns [`IssuerError::Storage`].
    pub fn record_issued(
        &mut self,
        course_id: &str,
        learner_key: &str,
        versions: usize,
        now: u64,
    ) -> Result<(), IssuerError> {
        self.connection.execute(
            "INSERT INTO issued_licenses (course_id, learner_key, issued_at, versions) VALUES (?1, ?2, ?3, ?4)",
            params![
                course_id,
                learner_key,
                seconds(now),
                i64::try_from(versions).unwrap_or(i64::MAX)
            ],
        )?;
        Ok(())
    }
}

fn seconds(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

impl From<rusqlite::Error> for IssuerError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn courses_belong_to_their_first_publisher_and_versions_keep_their_key() {
        let mut storage = Storage::open_in_memory().unwrap();
        storage.set_publisher("p1", "{}", "[]", 1).unwrap();
        storage.set_publisher("p2", "{}", "[]", 1).unwrap();
        assert_eq!(
            storage
                .register_release("p1", "c", "1.0.0", "k1", 10)
                .unwrap(),
            Registration::Added
        );
        assert_eq!(
            storage
                .register_release("p1", "c", "1.0.0", "k1", 11)
                .unwrap(),
            Registration::Unchanged
        );
        assert!(matches!(
            storage.register_release("p1", "c", "1.0.0", "k2", 12),
            Err(IssuerError::VersionRegistered)
        ));
        assert!(matches!(
            storage.register_release("p2", "c", "2.0.0", "k3", 12),
            Err(IssuerError::CourseOwned)
        ));
        storage
            .register_release("p1", "c", "1.1.0", "k4", 20)
            .unwrap();
        let versions: Vec<_> = storage
            .releases("c")
            .unwrap()
            .into_iter()
            .map(|release| release.course_version)
            .collect();
        assert_eq!(versions, ["1.0.0", "1.1.0"]);
        assert_eq!(
            storage.course_publisher("c").unwrap().as_deref(),
            Some("p1")
        );
    }

    #[test]
    fn entitlements_are_kept_once_per_course_and_learner() {
        let mut storage = Storage::open_in_memory().unwrap();
        assert!(!storage.entitled("c", "l").unwrap());
        assert!(storage.grant("c", "l", "manual", 1).unwrap());
        assert!(!storage.grant("c", "l", "manual", 2).unwrap());
        assert!(storage.entitled("c", "l").unwrap());
        assert!(!storage.entitled("c", "other").unwrap());
    }
}
