//! One-time import of the separate Learner and Publisher apps' data into the
//! one Zeron app's data directory (ADR 0011).
//!
//! The import is crash-safe and idempotent. Databases are copied, verified,
//! and adjusted in a staging directory; then a journal records the switch,
//! large directories move with `rename`, and the copies take their place.
//! Only after the switch are the old database files removed. Any restart
//! resumes from the journal, and a finished import does nothing again.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Data directory name of the old Learner app.
pub const LEARNER_DIRECTORY: &str = "app.zeron.learner";
/// Data directory name of the old Publisher app.
pub const PUBLISHER_DIRECTORY: &str = "app.zeron.publisher";
/// Records the import's progress inside the new data directory.
pub const JOURNAL_FILE: &str = "data-import.json";
/// Holds verified copies before they are switched in.
const STAGING_DIRECTORY: &str = ".data-import";

const LIBRARY_DATABASE: &str = "library.sqlite3";
const PUBLISHER_DATABASE: &str = "publisher.sqlite3";
/// Directories that move with the Learner's database.
const LIBRARY_DIRECTORIES: [&str; 1] = ["courses"];
/// Directories that move with the Publisher's database. Release directories
/// are read-only; moving their parent keeps their permissions.
const PUBLISHER_DIRECTORIES: [&str; 2] = ["releases", "staging"];

/// The Publisher's network choices, carried over when Library has none.
const NETWORK_KEYS: [&str; 3] = ["network_profile", "peer_port", "port_forwarding"];
/// The Publisher's seeding upload cap, carried over when Library has none.
const UPLOAD_LIMIT_KEY: &str = "upload_limit_kib";

/// Where to import from and to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPlan {
    /// The one app's data directory.
    pub target: PathBuf,
    /// The old Learner's data directory.
    pub learner: PathBuf,
    /// The old Publisher's data directory.
    pub publisher: PathBuf,
}

impl ImportPlan {
    /// The old apps' directories beside `target`, as every platform places
    /// app data directories side by side under one parent.
    #[must_use]
    pub fn beside(target: &Path) -> Self {
        Self {
            target: target.to_path_buf(),
            learner: target.with_file_name(LEARNER_DIRECTORY),
            publisher: target.with_file_name(PUBLISHER_DIRECTORY),
        }
    }
}

/// What an import run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportOutcome {
    /// An earlier run finished; nothing was touched.
    AlreadyDone,
    /// No old data was found; the journal now says so.
    NothingToImport,
    /// Old data was imported, or an interrupted import was finished.
    Imported {
        /// The Learner's library and courses were imported.
        learner: bool,
        /// The Publisher's drafts, releases, and staging were imported.
        publisher: bool,
    },
}

/// Why the import stopped. Nothing is lost: a later run resumes.
#[derive(Debug, Error)]
pub enum ImportError {
    /// A file operation failed.
    #[error("{step} failed: {source}")]
    Io {
        /// What was being done.
        step: &'static str,
        /// The underlying failure.
        source: io::Error,
    },
    /// An old or copied database could not be read or written.
    #[error("database copy failed: {0}")]
    Database(#[from] rusqlite::Error),
    /// The copied library could not be adjusted.
    #[error("library copy failed: {0}")]
    Library(#[from] learning_storage::StorageError),
    /// The copied publisher data could not be read.
    #[error("publisher copy failed: {0}")]
    Publisher(#[from] publisher_storage::PublisherStorageError),
    /// A copy did not match its original.
    #[error("the copied {0} did not verify")]
    Verification(&'static str),
    /// The new directory already holds something the import would replace.
    #[error("the new data directory already has {0}; nothing was replaced")]
    Conflict(&'static str),
    /// A path cannot be given to `SQLite`.
    #[error("a data directory path is not valid UTF-8")]
    UnsupportedPath,
    /// The journal is unreadable.
    #[error("the import journal is unreadable: {0}")]
    Journal(#[from] serde_json::Error),
}

fn io(step: &'static str) -> impl FnOnce(io::Error) -> ImportError {
    move |source| ImportError::Io { step, source }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    /// Verified copies are staged; moves and renames may be half done.
    Switching,
    /// Everything is in place; old database files may remain.
    Cleaning,
    /// Finished.
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Journal {
    version: u32,
    stage: Stage,
    /// The old Learner's database and courses are part of this import.
    learner: bool,
    /// The old Publisher's database, releases, and staging are part of it.
    publisher: bool,
}

/// Import the old apps' data into `plan.target`, or finish an import a
/// crash interrupted. Call before anything opens the target's databases.
///
/// # Errors
///
/// See [`ImportError`]. The target's databases are only ever replaced by
/// verified copies, and old databases are removed only after the switch.
pub fn import_old_data(plan: &ImportPlan) -> Result<ImportOutcome, ImportError> {
    fs::create_dir_all(&plan.target).map_err(io("creating the data directory"))?;
    let journal = match read_journal(&plan.target)? {
        Some(journal) if journal.stage == Stage::Done => return Ok(ImportOutcome::AlreadyDone),
        Some(journal) => journal,
        None => {
            let Some(journal) = stage(plan)? else {
                write_journal(
                    &plan.target,
                    Journal {
                        version: 1,
                        stage: Stage::Done,
                        learner: false,
                        publisher: false,
                    },
                )?;
                return Ok(ImportOutcome::NothingToImport);
            };
            journal
        }
    };
    finish(plan, journal)?;
    Ok(ImportOutcome::Imported {
        learner: journal.learner,
        publisher: journal.publisher,
    })
}

/// Copy, verify, and adjust the old databases in the staging directory,
/// then record the switch. `None` when there is nothing to import.
fn stage(plan: &ImportPlan) -> Result<Option<Journal>, ImportError> {
    let learner = is_file(&plan.learner.join(LIBRARY_DATABASE))
        && !exists(&plan.target.join(LIBRARY_DATABASE));
    let publisher = is_file(&plan.publisher.join(PUBLISHER_DATABASE))
        && !exists(&plan.target.join(PUBLISHER_DATABASE));
    if !learner && !publisher {
        return Ok(None);
    }
    let staging = plan.target.join(STAGING_DIRECTORY);
    remove_staging(&staging)?;
    fs::create_dir(&staging).map_err(io("creating the staging directory"))?;

    let staged_library = staging.join(LIBRARY_DATABASE);
    let staged_publisher = staging.join(PUBLISHER_DATABASE);
    if learner {
        copy_database(
            &plan.learner.join(LIBRARY_DATABASE),
            &staged_library,
            "library",
        )?;
    }
    if publisher {
        copy_database(
            &plan.publisher.join(PUBLISHER_DATABASE),
            &staged_publisher,
            "publisher database",
        )?;
    }
    let carried = if publisher {
        carried_settings(&staged_publisher)?
    } else {
        Vec::new()
    };
    // A library is staged when the Learner had one, or to carry the
    // Publisher's network settings into a new one.
    if learner || !carried.is_empty() {
        let mut library = learning_storage::Storage::open(&staged_library)?;
        if learner {
            library.rebase_locations(&plan.learner, &plan.target)?;
        }
        let has_network = NETWORK_KEYS
            .iter()
            .map(|key| library.setting(key))
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(Option::is_some);
        for (key, value) in &carried {
            let missing = if NETWORK_KEYS.contains(&key.as_str()) {
                !has_network
            } else {
                library.setting(key)?.is_none()
            };
            if missing {
                library.set_setting(key, value)?;
            }
        }
        drop(library);
        seal(&staged_library, "library")?;
    }
    if publisher {
        seal(&staged_publisher, "publisher database")?;
    }
    let journal = Journal {
        version: 1,
        stage: Stage::Switching,
        learner,
        publisher,
    };
    write_journal(&plan.target, journal)?;
    Ok(Some(journal))
}

/// Move directories and switch in the staged databases, then remove the old
/// database files. Every step is skipped when already done.
fn finish(plan: &ImportPlan, mut journal: Journal) -> Result<(), ImportError> {
    let staging = plan.target.join(STAGING_DIRECTORY);
    if journal.stage == Stage::Switching {
        if journal.learner {
            for name in LIBRARY_DIRECTORIES {
                move_directory(&plan.learner, &plan.target, name)?;
            }
        }
        if journal.publisher {
            for name in PUBLISHER_DIRECTORIES {
                move_directory(&plan.publisher, &plan.target, name)?;
            }
        }
        for name in [LIBRARY_DATABASE, PUBLISHER_DATABASE] {
            let staged = staging.join(name);
            if !exists(&staged) {
                continue;
            }
            if exists(&plan.target.join(name)) {
                return Err(ImportError::Conflict(name_of(name)));
            }
            fs::rename(&staged, plan.target.join(name)).map_err(io("switching in a database"))?;
        }
        journal.stage = Stage::Cleaning;
        write_journal(&plan.target, journal)?;
    }
    if journal.stage == Stage::Cleaning {
        remove_staging(&staging)?;
        if journal.learner {
            remove_database_files(&plan.learner.join(LIBRARY_DATABASE))?;
        }
        if journal.publisher {
            remove_database_files(&plan.publisher.join(PUBLISHER_DATABASE))?;
        }
        journal.stage = Stage::Done;
        write_journal(&plan.target, journal)?;
    }
    Ok(())
}

/// A consistent copy, including anything still in the write-ahead log,
/// checked for integrity and for the same schema version.
fn copy_database(source: &Path, copy: &Path, what: &'static str) -> Result<(), ImportError> {
    let original = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let destination = copy.to_str().ok_or(ImportError::UnsupportedPath)?;
    original.execute("VACUUM INTO ?1", [destination])?;
    let version: i64 = original.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    drop(original);
    let copied = Connection::open_with_flags(copy, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let copied_version: i64 = copied.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if copied_version != version || !intact(&copied)? {
        return Err(ImportError::Verification(what));
    }
    Ok(())
}

/// Leave a staged database as one self-contained, intact file.
fn seal(path: &Path, what: &'static str) -> Result<(), ImportError> {
    let connection = Connection::open(path)?;
    let mode: String =
        connection.query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))?;
    if mode != "delete" || !intact(&connection)? {
        return Err(ImportError::Verification(what));
    }
    drop(connection);
    if exists(&sidecar(path, "-wal")) {
        return Err(ImportError::Verification(what));
    }
    Ok(())
}

fn intact(connection: &Connection) -> Result<bool, ImportError> {
    let result: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    Ok(result == "ok")
}

/// Settings from the Publisher's copy that Library may take over.
fn carried_settings(publisher: &Path) -> Result<Vec<(String, String)>, ImportError> {
    let storage = publisher_storage::PublisherStorage::open(publisher)?;
    let mut carried = Vec::new();
    for key in NETWORK_KEYS.into_iter().chain([UPLOAD_LIMIT_KEY]) {
        if let Some(value) = storage.setting(key)? {
            carried.push((key.to_owned(), value));
        }
    }
    Ok(carried)
}

/// Move `from/name` to `to/name` with one rename. An empty directory at the
/// destination is replaced; anything else there stops the import.
fn move_directory(from: &Path, to: &Path, name: &'static str) -> Result<(), ImportError> {
    let source = from.join(name);
    let destination = to.join(name);
    if !exists(&source) {
        return Ok(());
    }
    if exists(&destination) {
        let empty = fs::read_dir(&destination)
            .map_err(io("checking the new data directory"))?
            .next()
            .is_none();
        if !empty || !is_real_directory(&destination) {
            return Err(ImportError::Conflict(name));
        }
        fs::remove_dir(&destination).map_err(io("replacing an empty directory"))?;
    }
    fs::rename(&source, &destination).map_err(io("moving a data directory"))
}

/// Remove a database and its `SQLite` side files.
fn remove_database_files(database: &Path) -> Result<(), ImportError> {
    for path in [
        database.to_path_buf(),
        sidecar(database, "-wal"),
        sidecar(database, "-shm"),
        sidecar(database, "-journal"),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io("removing an old database")(error)),
        }
    }
    Ok(())
}

/// Remove the staging directory, which only this import writes. A symlink
/// in its place is removed as a link and never followed.
fn remove_staging(staging: &Path) -> Result<(), ImportError> {
    match fs::symlink_metadata(staging) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io("checking the staging directory")(error)),
        Ok(metadata) if metadata.is_dir() => {
            fs::remove_dir_all(staging).map_err(io("removing the staging directory"))
        }
        Ok(_) => fs::remove_file(staging).map_err(io("removing the staging directory")),
    }
}

fn read_journal(target: &Path) -> Result<Option<Journal>, ImportError> {
    match fs::read(target.join(JOURNAL_FILE)) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io("reading the import journal")(error)),
    }
}

/// Replace the journal atomically: write a temporary file, flush it, rename.
fn write_journal(target: &Path, journal: Journal) -> Result<(), ImportError> {
    let temporary = target.join(format!("{JOURNAL_FILE}.tmp"));
    let bytes = serde_json::to_vec(&journal)?;
    {
        use io::Write as _;
        let mut file = fs::File::create(&temporary).map_err(io("writing the import journal"))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(io("writing the import journal"))?;
    }
    fs::rename(&temporary, target.join(JOURNAL_FILE)).map_err(io("writing the import journal"))?;
    sync_directory(target);
    Ok(())
}

/// Best effort: make renames in `directory` durable where the platform can.
fn sync_directory(directory: &Path) {
    #[cfg(unix)]
    if let Ok(handle) = fs::File::open(directory) {
        let _ = handle.sync_all();
    }
    #[cfg(not(unix))]
    let _ = directory;
}

fn sidecar(database: &Path, suffix: &str) -> PathBuf {
    let mut name = database.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn name_of(database: &str) -> &'static str {
    if database == LIBRARY_DATABASE {
        "a library"
    } else {
        "publisher data"
    }
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn is_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

fn is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}
