//! Removing a course from the library while keeping the learner's own work
//! (ADR 0015).
//!
//! A local folder import is the learner's folder: removal only forgets it.
//! A torrent course's download directory belongs to Zeron and is deleted in
//! two steps that survive a crash: it is renamed to `<info hash>.deleting`
//! before the course is marked removed, then deleted. Start-up finishes any
//! `.deleting` directory left behind. Links are never followed.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use course_format::CourseManifest;
use learning_storage::{StoredCourseSummary, TorrentCourseSource};
use torrent_port::InfoHash;

use crate::{CourseLibrary, LibraryError};

/// Suffix of a download directory whose course was removed.
pub const DELETING_SUFFIX: &str = ".deleting";

impl CourseLibrary {
    /// Remove a course: it leaves the library, search, and start-up restore,
    /// and its notes, drawings, progress, and positions are kept for a later
    /// import of the same course. Returns `false` for an unknown or already
    /// removed course. Files are handled by [`set_aside_course_files`].
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on failure; nothing changes then.
    pub fn remove_course(&mut self, course_id: &str) -> Result<bool, LibraryError> {
        Ok(self.storage.remove_course(course_id)?.is_some())
    }

    /// Courses the learner removed whose work is kept.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn removed_courses(&self) -> Result<Vec<StoredCourseSummary>, LibraryError> {
        Ok(self.storage.removed_courses()?)
    }

    /// The manifest of a course in the library or removed from it, to show a
    /// removed course's notes in lesson order.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query or decoding failure.
    pub fn kept_manifest(&self, course_id: &str) -> Result<Option<CourseManifest>, LibraryError> {
        Ok(self.storage.kept_course_manifest(course_id)?)
    }
}

/// A torrent course's download directory, renamed out of the way and waiting
/// to be deleted or, if removal fails, put back.
#[derive(Debug)]
pub struct SetAsideFiles {
    original: PathBuf,
    deleting: PathBuf,
}

impl SetAsideFiles {
    /// Delete the directory and everything in it. Links inside are removed,
    /// not followed.
    ///
    /// # Errors
    ///
    /// The I/O failure; the `.deleting` directory is then finished at start-up.
    pub fn delete(self) -> io::Result<()> {
        match fs::remove_dir_all(&self.deleting) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// Put the directory back, after the course could not be removed.
    ///
    /// # Errors
    ///
    /// The I/O failure of the rename.
    pub fn restore(self) -> io::Result<()> {
        fs::rename(&self.deleting, &self.original)
    }
}

/// Rename a removed torrent course's download directory out of the way.
///
/// Only `courses_root/<info hash>` is touched, only when it is the source's
/// recorded directory, a real directory rather than a link, and no other
/// course uses the same torrent. Returns `None` when there is nothing Zeron
/// may delete.
///
/// # Errors
///
/// The I/O failure of the rename.
pub fn set_aside_course_files(
    courses_root: &Path,
    source: &TorrentCourseSource,
    sources: &[TorrentCourseSource],
) -> io::Result<Option<SetAsideFiles>> {
    let Ok(info_hash) = InfoHash::parse(&source.info_hash) else {
        return Ok(None);
    };
    let original = courses_root.join(info_hash.as_str());
    let shared = sources
        .iter()
        .any(|other| other.course_id != source.course_id && other.info_hash == source.info_hash);
    if source.content_root != original || shared {
        return Ok(None);
    }
    match fs::symlink_metadata(&original) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    let deleting = courses_root.join(format!("{}{DELETING_SUFFIX}", info_hash.as_str()));
    // A deletion an earlier crash interrupted for the same torrent goes first.
    finish_removal(&deleting)?;
    fs::rename(&original, &deleting)?;
    Ok(Some(SetAsideFiles { original, deleting }))
}

/// Delete every `<info hash>.deleting` directory a crash left in
/// `courses_root`. Anything else, and anything that is not a real directory,
/// is left alone. Returns how many were deleted.
///
/// # Errors
///
/// The first I/O failure other than a missing root.
pub fn finish_interrupted_removals(courses_root: &Path) -> io::Result<usize> {
    let entries = match fs::read_dir(courses_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut finished = 0;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(hash) = name
            .to_str()
            .and_then(|name| name.strip_suffix(DELETING_SUFFIX))
        else {
            continue;
        };
        if InfoHash::parse(hash).is_err() {
            continue;
        }
        if finish_removal(&entry.path())? {
            finished += 1;
        }
    }
    Ok(finished)
}

/// Delete a `.deleting` directory if it is a real directory.
fn finish_removal(deleting: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(deleting) {
        Ok(metadata) if metadata.is_dir() => {
            fs::remove_dir_all(deleting)?;
            Ok(true)
        }
        Ok(_) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}
