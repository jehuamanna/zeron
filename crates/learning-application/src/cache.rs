//! Freeing disk space taken by streamed lessons the learner does not keep.
//!
//! Only learner-downloaded lesson files below `courses_root/<info hash>/` are
//! ever removed: never `course.json`, never kept lessons, never anything a
//! symbolic link points at. A running torrent believes its pieces are still
//! verified, so a course's transfer is stopped before its files are removed
//! and added again afterwards; its initial check then sees what is gone.

use std::{
    collections::{BTreeSet, HashMap},
    fs, io,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use learning_storage::TorrentCourseSource;
use thiserror::Error;
use torrent_port::{EngineError, FetchRequest, InfoHash, TorrentEngine};

use crate::{CourseLibrary, LibraryError, torrent_import::MANIFEST_PATH};

/// What a removal freed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemovalReport {
    /// Lesson resources whose files were removed.
    pub removed: Vec<String>,
    /// Verified bytes those files held before removal.
    pub freed_bytes: u64,
    /// Requested resources left alone because the learner keeps them offline.
    pub kept: Vec<String>,
}

/// Cache maintenance failure.
#[derive(Debug, Error)]
pub enum CacheError {
    /// The course is not a torrent course in this library.
    #[error("only courses imported from a torrent have removable downloads")]
    NotTorrentCourse,
    /// The library could not be read or written.
    #[error(transparent)]
    Library(#[from] LibraryError),
    /// The engine could not stop or restart the course's transfer.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// A file could not be removed. The message never names a path.
    #[error("a downloaded lesson file could not be removed: {0}")]
    Io(io::ErrorKind),
}

/// Removes learner-downloaded lesson data, by request or to honor a size cap.
pub struct CourseCache<E> {
    engine: Arc<E>,
    library: Arc<Mutex<CourseLibrary>>,
    courses_root: PathBuf,
}

struct Removal {
    source: TorrentCourseSource,
    selection: Vec<usize>,
    files: Vec<(String, PathBuf, u64)>,
    kept: Vec<String>,
}

impl<E: TorrentEngine> CourseCache<E> {
    /// Compose the cache over the engine that holds learner downloads below
    /// `courses_root/<info hash>/`.
    #[must_use]
    pub fn new(engine: Arc<E>, library: Arc<Mutex<CourseLibrary>>, courses_root: PathBuf) -> Self {
        Self {
            engine,
            library,
            courses_root,
        }
    }

    /// Remove the downloaded files of `resources` (every lesson when `None`)
    /// that the learner does not keep offline.
    ///
    /// # Errors
    /// Returns [`CacheError::NotTorrentCourse`], an engine failure while
    /// stopping or restarting the transfer, or an I/O failure.
    pub async fn remove(
        &self,
        course_id: &str,
        resources: Option<&[String]>,
    ) -> Result<RemovalReport, CacheError> {
        let removal = self.plan(course_id, resources)?;
        let info_hash = InfoHash::parse(&removal.source.info_hash)?;
        let verified = self
            .engine
            .transfer(&info_hash)
            .map(|stats| stats.file_verified_bytes)
            .unwrap_or_default();
        let mut report = RemovalReport {
            kept: removal.kept.clone(),
            ..RemovalReport::default()
        };
        if removal.files.is_empty() {
            return Ok(report);
        }
        let was_active = match self.engine.stop(&info_hash).await {
            Ok(()) => true,
            Err(EngineError::NotActive) => false,
            Err(error) => return Err(error.into()),
        };
        let removed = remove_files(&removal.source.content_root, &removal.files, &verified);
        if was_active {
            self.engine
                .fetch(FetchRequest {
                    metainfo: removal.source.metainfo.clone(),
                    content_root: removal.source.content_root.clone(),
                    files: removal.selection.clone(),
                    initial_peers: Vec::new(),
                })
                .await?;
            if removal.source.paused {
                self.engine.pause(&info_hash).await?;
            }
        }
        let (removed, freed_bytes) = removed?;
        report.removed = removed;
        report.freed_bytes = freed_bytes;
        Ok(report)
    }

    /// Bytes of streamed lessons that are not kept offline, across courses.
    ///
    /// # Errors
    /// Returns a library failure.
    pub fn streamed_bytes(&self) -> Result<u64, CacheError> {
        Ok(self
            .candidates(None)?
            .iter()
            .map(|candidate| candidate.bytes)
            .sum())
    }

    /// The stored cap on streamed lessons, in bytes.
    ///
    /// # Errors
    /// Returns a library failure.
    pub fn limit(&self) -> Result<Option<u64>, CacheError> {
        Ok(self.library().cache_limit()?)
    }

    /// Remove least recently played lessons that are not kept offline until
    /// the streamed total fits the stored cap. Lessons of `skip_course`, such
    /// as the course being played, are never touched.
    ///
    /// # Errors
    /// Returns a library, engine, or I/O failure.
    pub async fn enforce_limit(
        &self,
        skip_course: Option<&str>,
    ) -> Result<RemovalReport, CacheError> {
        let Some(limit) = self.limit()? else {
            return Ok(RemovalReport::default());
        };
        let candidates = self.candidates(skip_course)?;
        let mut total: u64 = self.streamed_bytes()?;
        let mut chosen: HashMap<String, Vec<String>> = HashMap::new();
        for candidate in candidates {
            if total <= limit {
                break;
            }
            total = total.saturating_sub(candidate.bytes);
            chosen
                .entry(candidate.course_id)
                .or_default()
                .push(candidate.resource_id);
        }
        let mut report = RemovalReport::default();
        for (course_id, resources) in chosen {
            let removed = self.remove(&course_id, Some(&resources)).await?;
            report.removed.extend(removed.removed);
            report.freed_bytes += removed.freed_bytes;
        }
        Ok(report)
    }

    fn plan(&self, course_id: &str, resources: Option<&[String]>) -> Result<Removal, CacheError> {
        let library = self.library();
        let source = library
            .torrent_source(course_id)?
            .ok_or(CacheError::NotTorrentCourse)?;
        let expected_root = self.courses_root.join(&source.info_hash);
        if source.content_root != expected_root {
            // Only content the import service created below the courses root
            // is ever removed.
            return Err(CacheError::NotTorrentCourse);
        }
        let files = torrent_metainfo::file_table(&source.metainfo)
            .map_err(|_| CacheError::NotTorrentCourse)?;
        let committed = library.committed_resources(course_id)?;
        let selection = library.download_selection(course_id)?.unwrap_or_default();
        let wanted: Option<BTreeSet<&String>> = resources.map(|ids| ids.iter().collect());
        let mut removal = Removal {
            selection,
            files: Vec::new(),
            kept: Vec::new(),
            source,
        };
        for resource in library.torrent_resources(course_id)? {
            if wanted
                .as_ref()
                .is_some_and(|wanted| !wanted.contains(&resource.resource_id))
            {
                continue;
            }
            if committed.contains(&resource.resource_id) {
                removal.kept.push(resource.resource_id);
                continue;
            }
            let Some(file) = files.get(resource.file_index) else {
                continue;
            };
            if file.path == MANIFEST_PATH {
                continue;
            }
            if let Some(relative) = relative_path(&file.path) {
                removal.files.push((
                    resource.resource_id,
                    relative,
                    u64::try_from(resource.file_index).unwrap_or(u64::MAX),
                ));
            }
        }
        Ok(removal)
    }

    fn candidates(&self, skip_course: Option<&str>) -> Result<Vec<Candidate>, CacheError> {
        let library = self.library();
        let mut candidates = Vec::new();
        for source in library.torrent_sources()? {
            if Some(source.course_id.as_str()) == skip_course {
                continue;
            }
            let Ok(info_hash) = InfoHash::parse(&source.info_hash) else {
                continue;
            };
            let Some(stats) = self.engine.transfer(&info_hash) else {
                continue;
            };
            let committed = library.committed_resources(&source.course_id)?;
            let recency = library.playback_recency(&source.course_id)?;
            for resource in library.torrent_resources(&source.course_id)? {
                let bytes = stats
                    .file_verified_bytes
                    .get(resource.file_index)
                    .copied()
                    .unwrap_or(0);
                if bytes == 0 || committed.contains(&resource.resource_id) {
                    continue;
                }
                candidates.push(Candidate {
                    last_played: recency.get(&resource.resource_id).copied().unwrap_or(0),
                    course_id: source.course_id.clone(),
                    resource_id: resource.resource_id,
                    bytes,
                });
            }
        }
        candidates.sort_by(|a, b| {
            a.last_played
                .cmp(&b.last_played)
                .then_with(|| b.bytes.cmp(&a.bytes))
        });
        Ok(candidates)
    }

    fn library(&self) -> MutexGuard<'_, CourseLibrary> {
        self.library.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct Candidate {
    course_id: String,
    resource_id: String,
    bytes: u64,
    last_played: u64,
}

/// A torrent-declared path as normal relative components, or `None`.
fn relative_path(path: &str) -> Option<PathBuf> {
    let relative = PathBuf::from(path);
    (!path.is_empty()
        && !path.contains(['\\', ':', '\0'])
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_))))
    .then_some(relative)
}

/// Remove each file below `root` without following links: every directory on
/// the way must be a real directory, and the last component a regular file.
/// Returns the removed resource IDs and the verified bytes they held.
fn remove_files(
    root: &Path,
    files: &[(String, PathBuf, u64)],
    verified: &[u64],
) -> Result<(Vec<String>, u64), CacheError> {
    let mut removed = Vec::new();
    let mut freed = 0_u64;
    for (resource_id, relative, index) in files {
        let Some(target) = contained_file(root, relative) else {
            continue;
        };
        match fs::remove_file(&target) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(CacheError::Io(error.kind())),
        }
        freed += usize::try_from(*index)
            .ok()
            .and_then(|index| verified.get(index))
            .copied()
            .unwrap_or(0);
        removed.push(resource_id.clone());
    }
    Ok((removed, freed))
}

fn contained_file(root: &Path, relative: &Path) -> Option<PathBuf> {
    if !fs::symlink_metadata(root).ok()?.is_dir() {
        return None;
    }
    let mut current = root.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current).ok()?;
        let last = components.peek().is_none();
        if (last && !metadata.is_file()) || (!last && !metadata.is_dir()) {
            return None;
        }
    }
    Some(current)
}
