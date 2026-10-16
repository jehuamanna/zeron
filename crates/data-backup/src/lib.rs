//! Backing up and restoring Zeron's databases.
//!
//! A backup is one file: a magic line, a JSON header naming each database
//! with its length and SHA-256, then the databases themselves. Each database
//! is a consistent snapshot taken with `VACUUM INTO` while the app runs.
//!
//! Restoring happens in two steps, because the running app holds the
//! databases open:
//! - [`stage_restore`] verifies the backup and stages its databases.
//! - [`apply_pending_restore`] runs at the next start, before anything opens
//!   a database. It moves the current databases and their journals aside
//!   into `before-restore-<time>/` and moves the staged ones in.
//!
//! Each step is crash-safe: a restart repeats or finishes it. Course
//! packages, downloaded torrents, release files, and signing keys are not
//! part of a backup.

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// First line of every backup file.
pub const MAGIC: &[u8] = b"ZERON-BACKUP\n";
/// The `format` value every backup header carries.
pub const FORMAT: &str = "zeron.backup";
const VERSION: u32 = 1;
const MAX_HEADER_BYTES: u32 = 64 * 1024;
/// Largest database a backup may hold.
pub const MAX_DATABASE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const SNAPSHOT_DIRECTORY: &str = ".backup-snapshots";
const STAGING_DIRECTORY: &str = ".restore-staging";
const PENDING_FILE: &str = "restore-pending.json";
const JOURNAL_SUFFIXES: [&str; 2] = ["-wal", "-shm"];

/// A database Zeron backs up: its file name in the data directory and the
/// newest schema version this build can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Database {
    /// File name, such as `library.sqlite3`.
    pub name: &'static str,
    /// Newest `user_version` this build understands.
    pub schema_version: i64,
}

/// One database in a backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupEntry {
    /// File name in the data directory.
    pub name: String,
    /// Exact length in bytes.
    pub length: u64,
    /// Lowercase hex SHA-256.
    pub sha256: String,
    /// Its `user_version`.
    pub schema_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Header {
    format: String,
    version: u32,
    created_at: u64,
    entries: Vec<BackupEntry>,
}

/// What a backup holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupSummary {
    /// When it was made, in Unix seconds.
    pub created_at: u64,
    /// The databases in it.
    pub entries: Vec<BackupEntry>,
}

/// Why a backup or restore failed. Messages name no paths.
#[derive(Debug, Error)]
pub enum BackupError {
    /// Reading or writing failed.
    #[error("{operation} failed: {kind}")]
    Io {
        /// What was being done.
        operation: &'static str,
        /// The kind of I/O failure.
        kind: io::ErrorKind,
    },
    /// `SQLite` refused a snapshot or check.
    #[error("the database could not be {0}")]
    Database(&'static str),
    /// The file is not a Zeron backup, or it was changed.
    #[error("this file is not a Zeron backup, or it was changed: {0}")]
    Invalid(&'static str),
    /// The backup was made by a newer Zeron.
    #[error("this backup was made by a newer version of Zeron; update Zeron first")]
    TooNew,
}

fn io_error(operation: &'static str) -> impl FnOnce(io::Error) -> BackupError {
    move |error| BackupError::Io {
        operation,
        kind: error.kind(),
    }
}

/// Write a backup of every database in `databases` that exists under
/// `data_dir` to `destination`, taking each snapshot with `VACUUM INTO`.
///
/// # Errors
/// Returns a database or I/O failure; nothing partial is left in `data_dir`.
pub fn write_backup(
    data_dir: &Path,
    databases: &[Database],
    created_at: u64,
    destination: &mut dyn Write,
) -> Result<BackupSummary, BackupError> {
    let snapshots = data_dir.join(SNAPSHOT_DIRECTORY);
    let _ = fs::remove_dir_all(&snapshots);
    fs::create_dir_all(&snapshots).map_err(io_error("preparing the backup"))?;
    let result = write_snapshots(data_dir, &snapshots, databases, created_at, destination);
    let _ = fs::remove_dir_all(&snapshots);
    result
}

fn write_snapshots(
    data_dir: &Path,
    snapshots: &Path,
    databases: &[Database],
    created_at: u64,
    destination: &mut dyn Write,
) -> Result<BackupSummary, BackupError> {
    let mut entries = Vec::new();
    let mut files = Vec::new();
    for database in databases {
        let live = data_dir.join(database.name);
        if !live.is_file() {
            continue;
        }
        let snapshot = snapshots.join(database.name);
        let connection = Connection::open_with_flags(&live, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| BackupError::Database("opened"))?;
        connection
            .execute("VACUUM INTO ?1", [snapshot.to_string_lossy().as_ref()])
            .map_err(|_| BackupError::Database("snapshotted"))?;
        drop(connection);
        let (length, sha256) = hash_file(&snapshot)?;
        entries.push(BackupEntry {
            name: database.name.to_owned(),
            length,
            sha256,
            schema_version: schema_version(&snapshot)?,
        });
        files.push(snapshot);
    }
    let header = serde_json::to_vec(&Header {
        format: FORMAT.to_owned(),
        version: VERSION,
        created_at,
        entries: entries.clone(),
    })
    .map_err(|_| BackupError::Invalid("the header could not be written"))?;
    let header_length =
        u32::try_from(header.len()).map_err(|_| BackupError::Invalid("the header is too large"))?;
    let write = io_error("writing the backup");
    destination.write_all(MAGIC).map_err(write)?;
    destination
        .write_all(&header_length.to_le_bytes())
        .map_err(io_error("writing the backup"))?;
    destination
        .write_all(&header)
        .map_err(io_error("writing the backup"))?;
    for file in files {
        let mut source = File::open(&file).map_err(io_error("reading a snapshot"))?;
        io::copy(&mut source, destination).map_err(io_error("writing the backup"))?;
    }
    destination
        .flush()
        .map_err(io_error("writing the backup"))?;
    Ok(BackupSummary {
        created_at,
        entries,
    })
}

/// Verify a backup and stage its databases for the next start. A backup of
/// a database with a schema newer than `databases` allow is refused, so
/// Zeron never restores something it cannot open.
///
/// # Errors
/// Returns [`BackupError::Invalid`], [`BackupError::TooNew`], or a database
/// or I/O failure; nothing is staged on failure.
pub fn stage_restore(
    data_dir: &Path,
    databases: &[Database],
    source: &mut dyn Read,
) -> Result<BackupSummary, BackupError> {
    let staging = data_dir.join(STAGING_DIRECTORY);
    discard_pending_restore(data_dir)?;
    fs::create_dir_all(&staging).map_err(io_error("preparing the restore"))?;
    let result = stage_entries(&staging, databases, source);
    match result {
        Ok(summary) => {
            write_atomically(
                &data_dir.join(PENDING_FILE),
                &serde_json::to_vec(&summary.entries)
                    .map_err(|_| BackupError::Invalid("the restore could not be recorded"))?,
            )?;
            Ok(summary)
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            Err(error)
        }
    }
}

fn stage_entries(
    staging: &Path,
    databases: &[Database],
    source: &mut dyn Read,
) -> Result<BackupSummary, BackupError> {
    let mut magic = [0_u8; MAGIC.len()];
    source
        .read_exact(&mut magic)
        .map_err(|_| BackupError::Invalid("it does not start like one"))?;
    if magic != MAGIC {
        return Err(BackupError::Invalid("it does not start like one"));
    }
    let mut length = [0_u8; 4];
    source
        .read_exact(&mut length)
        .map_err(|_| BackupError::Invalid("its header is cut short"))?;
    let length = u32::from_le_bytes(length);
    if length > MAX_HEADER_BYTES {
        return Err(BackupError::Invalid("its header is too large"));
    }
    let mut header = vec![0_u8; length as usize];
    source
        .read_exact(&mut header)
        .map_err(|_| BackupError::Invalid("its header is cut short"))?;
    let header: Header = serde_json::from_slice(&header)
        .map_err(|_| BackupError::Invalid("its header cannot be read"))?;
    if header.format != FORMAT || header.version != VERSION || header.entries.is_empty() {
        return Err(BackupError::Invalid("it is of another format or version"));
    }
    let mut seen = Vec::new();
    for entry in &header.entries {
        let database = databases
            .iter()
            .find(|database| database.name == entry.name)
            .ok_or(BackupError::Invalid("it holds a file Zeron does not use"))?;
        if seen.contains(&entry.name) || entry.length > MAX_DATABASE_BYTES {
            return Err(BackupError::Invalid("its list of databases is malformed"));
        }
        seen.push(entry.name.clone());
        if entry.schema_version > database.schema_version {
            return Err(BackupError::TooNew);
        }
        let partial = staging.join(format!("{}.partial", entry.name));
        let mut file = File::create(&partial).map_err(io_error("staging a database"))?;
        let copied = io::copy(&mut (&mut *source).take(entry.length), &mut file)
            .map_err(io_error("staging a database"))?;
        file.sync_all().map_err(io_error("staging a database"))?;
        drop(file);
        if copied != entry.length {
            return Err(BackupError::Invalid("a database is cut short"));
        }
        let (_, sha256) = hash_file(&partial)?;
        if sha256 != entry.sha256 {
            return Err(BackupError::Invalid(
                "a database does not match its checksum",
            ));
        }
        if schema_version(&partial)? != entry.schema_version || !quick_check(&partial) {
            return Err(BackupError::Invalid("a database is damaged"));
        }
        fs::rename(&partial, staging.join(&entry.name)).map_err(io_error("staging a database"))?;
    }
    let mut extra = [0_u8; 1];
    if source
        .read(&mut extra)
        .map_err(io_error("reading the backup"))?
        != 0
    {
        return Err(BackupError::Invalid("it has data after its databases"));
    }
    Ok(BackupSummary {
        created_at: header.created_at,
        entries: header.entries,
    })
}

/// The restore waiting for the next start, if any.
///
/// # Errors
/// Returns an I/O failure.
pub fn pending_restore(data_dir: &Path) -> Result<Option<Vec<BackupEntry>>, BackupError> {
    match fs::read(data_dir.join(PENDING_FILE)) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error("reading the pending restore")(error)),
    }
}

/// Forget a staged restore.
///
/// # Errors
/// Returns an I/O failure.
pub fn discard_pending_restore(data_dir: &Path) -> Result<(), BackupError> {
    match fs::remove_file(data_dir.join(PENDING_FILE)) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error("discarding the restore")(error)),
    }
    match fs::remove_dir_all(data_dir.join(STAGING_DIRECTORY)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("discarding the restore")(error)),
    }
}

/// At startup, before any database opens: switch in a staged restore. The
/// current databases and their journals move into `before-restore-<now>/`.
/// Returns the folder they moved to, or `None` when nothing was pending.
/// Interrupted runs resume: every staged database still present is switched
/// in, and the record is removed last.
///
/// # Errors
/// Returns an I/O failure; the next start tries again.
pub fn apply_pending_restore(data_dir: &Path, now: u64) -> Result<Option<PathBuf>, BackupError> {
    let Some(entries) = pending_restore(data_dir)? else {
        return Ok(None);
    };
    let staging = data_dir.join(STAGING_DIRECTORY);
    let aside = data_dir.join(format!("before-restore-{now}"));
    fs::create_dir_all(&aside).map_err(io_error("keeping the current databases"))?;
    for entry in &entries {
        let staged = staging.join(&entry.name);
        if !staged.is_file() {
            // Switched in by an interrupted run.
            continue;
        }
        let live = data_dir.join(&entry.name);
        for suffix in std::iter::once("").chain(JOURNAL_SUFFIXES) {
            let name = format!("{}{suffix}", entry.name);
            let current = data_dir.join(&name);
            if current.exists() {
                fs::rename(&current, aside.join(&name))
                    .map_err(io_error("keeping the current databases"))?;
            }
        }
        fs::rename(&staged, &live).map_err(io_error("switching in the restored database"))?;
    }
    fs::remove_file(data_dir.join(PENDING_FILE)).map_err(io_error("finishing the restore"))?;
    let _ = fs::remove_dir_all(&staging);
    Ok(Some(aside))
}

fn hash_file(path: &Path) -> Result<(u64, String), BackupError> {
    let mut file = File::open(path).map_err(io_error("hashing a database"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut length = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(io_error("hashing a database"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        length += read as u64;
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    Ok((length, hex))
}

fn schema_version(path: &Path) -> Result<i64, BackupError> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|connection| connection.query_row("PRAGMA user_version", [], |row| row.get(0)))
        .map_err(|_| BackupError::Invalid("a database cannot be read"))
}

fn quick_check(path: &Path) -> bool {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|connection| {
            connection.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
        })
        .is_ok_and(|result| result == "ok")
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), BackupError> {
    let partial = path.with_extension("partial");
    let mut file = File::create(&partial).map_err(io_error("recording the restore"))?;
    file.write_all(bytes)
        .map_err(io_error("recording the restore"))?;
    file.sync_all().map_err(io_error("recording the restore"))?;
    drop(file);
    fs::rename(&partial, path).map_err(io_error("recording the restore"))
}
