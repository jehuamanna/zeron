use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use course_format::CourseManifest;
use learning_storage::{
    PlaybackPosition, Storage, StorageError, StoredCourseSummary, TorrentCourseSource,
};
use thiserror::Error;
use torrent_port::{NetworkProfile, PeerReachability, RateLimits};

use crate::{CancellationFlag, ImportError, ImportLimits, import_local_course};

const NETWORK_PROFILE_KEY: &str = "network_profile";
const PEER_PORT_KEY: &str = "peer_port";
const PORT_FORWARDING_KEY: &str = "port_forwarding";
const DOWNLOAD_LIMIT_KEY: &str = "download_limit_kib";
const UPLOAD_LIMIT_KEY: &str = "upload_limit_kib";
const CACHE_LIMIT_KEY: &str = "cache_limit_bytes";

/// Result of a verified and durably persisted local course import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseImportReceipt {
    /// Stable logical course ID.
    pub course_id: String,
    /// Imported course version.
    pub course_version: String,
    /// Display title.
    pub title: String,
    /// Number of verified resources.
    pub verified_resource_count: usize,
    /// Sum of verified resource bytes.
    pub total_resource_bytes: u64,
}

/// Backend-only description of a verified package resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalResource {
    /// Filesystem location. Never serialize this field over IPC.
    pub path: PathBuf,
    /// Manifest-declared media type.
    pub media_type: String,
    /// Manifest-declared byte length.
    pub byte_length: u64,
}

/// Where a torrent-sourced resource lives in its torrent. Backend-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentResource {
    /// Stable resource ID from the manifest.
    pub resource_id: String,
    /// Info hash of the course torrent.
    pub info_hash: String,
    /// Engine file index of the resource.
    pub file_index: usize,
    /// Manifest-declared media type.
    pub media_type: String,
    /// Manifest-declared byte length, equal to the torrent file length.
    pub byte_length: u64,
}

/// Course-library startup and use-case failures.
#[derive(Debug, Error)]
pub enum LibraryError {
    /// Local package validation or verification failed.
    #[error(transparent)]
    Import(#[from] ImportError),
    /// Durable storage failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// The selected package root cannot be resolved for durable recovery.
    #[error("cannot resolve the local course package root: {0}")]
    PackageLocation(#[source] std::io::Error),
    /// A playback position or duration is not a usable number of seconds.
    #[error("{0} must be a finite number of seconds within the lesson")]
    InvalidPlaybackTime(&'static str),
}

/// Longest lesson a playback position is recorded for.
const MAX_LESSON_SECONDS: f64 = 48.0 * 3600.0;
/// Positions this close to the end count as finished, so the lesson starts over.
const FINISHED_WITHIN_SECONDS: f64 = 10.0;

/// Ready local course-library application service.
///
/// This type composes verification and persistence while keeping both adapters
/// out of UI and Tauri command handlers.
pub struct CourseLibrary {
    pub(crate) storage: Storage,
    import_limits: ImportLimits,
}

impl CourseLibrary {
    /// Open, migrate, and capability-check a file-backed course library.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] until durable storage is ready.
    pub fn open(database_path: &Path, import_limits: ImportLimits) -> Result<Self, LibraryError> {
        Ok(Self {
            storage: Storage::open(database_path)?,
            import_limits,
        })
    }

    /// Construct a test/development library using isolated in-memory storage.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] if migrations or mandatory `SQLite`
    /// capabilities fail.
    pub fn open_in_memory(import_limits: ImportLimits) -> Result<Self, LibraryError> {
        Ok(Self {
            storage: Storage::open_in_memory()?,
            import_limits,
        })
    }

    /// Verify a local package completely, then persist its snapshot atomically.
    ///
    /// No course becomes visible when verification fails or is cancelled.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Import`] for package failures and
    /// [`LibraryError::Storage`] when durable persistence fails.
    pub fn import_local_folder(
        &mut self,
        root: &Path,
        cancellation: &CancellationFlag,
    ) -> Result<CourseImportReceipt, LibraryError> {
        let imported = import_local_course(root, self.import_limits, cancellation)?;
        let package_root = std::fs::canonicalize(root).map_err(LibraryError::PackageLocation)?;
        let receipt = CourseImportReceipt {
            course_id: imported.manifest.course_id.clone(),
            course_version: imported.manifest.version.clone(),
            title: imported.manifest.title.clone(),
            verified_resource_count: imported.resources.len(),
            total_resource_bytes: imported.total_resource_bytes,
        };
        self.storage.save_imported_course(
            &imported.manifest,
            imported.total_resource_bytes,
            &package_root,
        )?;
        Ok(receipt)
    }

    /// Persist a course whose manifest was validated from a torrent. Only
    /// `course.json` is verified; resources stay unavailable to playback.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] when persistence fails.
    pub fn save_torrent_course(
        &mut self,
        manifest: &CourseManifest,
        total_resource_bytes: u64,
        content_root: &Path,
        info_hash: &str,
        metainfo: &[u8],
    ) -> Result<StoredCourseSummary, LibraryError> {
        Ok(self.storage.save_torrent_course(
            manifest,
            total_resource_bytes,
            content_root,
            info_hash,
            metainfo,
        )?)
    }

    /// Torrent-sourced courses for engine restore. Paths stay in the backend.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn torrent_course_sources(&self) -> Result<Vec<TorrentCourseSource>, LibraryError> {
        Ok(self.storage.torrent_course_sources()?)
    }

    /// Info hash of a torrent-sourced course, or `None` for local imports.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn torrent_info_hash(&self, course_id: &str) -> Result<Option<String>, LibraryError> {
        Ok(self.storage.course_torrent_info_hash(course_id)?)
    }

    /// Store whether the learner shares a torrent-sourced course. Returns
    /// `false` when the course is not torrent-sourced.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on write failure.
    pub fn set_course_sharing(
        &mut self,
        course_id: &str,
        sharing: bool,
    ) -> Result<bool, LibraryError> {
        Ok(self.storage.set_course_sharing(course_id, sharing)?)
    }

    /// Record whether the learner paused a torrent course. Returns `false` for
    /// courses that are not torrent-sourced.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on write failure.
    pub fn set_course_paused(
        &mut self,
        course_id: &str,
        paused: bool,
    ) -> Result<bool, LibraryError> {
        Ok(self.storage.set_course_paused(course_id, paused)?)
    }

    /// Whether the learner paused a torrent course.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn course_paused(&self, course_id: &str) -> Result<bool, LibraryError> {
        Ok(self
            .storage
            .torrent_course_sources()?
            .iter()
            .any(|source| source.course_id == course_id && source.paused))
    }

    /// Configured network profile; local network unless the learner chose otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn network_profile(&self) -> Result<NetworkProfile, LibraryError> {
        Ok(self
            .storage
            .setting(NETWORK_PROFILE_KEY)?
            .as_deref()
            .and_then(NetworkProfile::from_name)
            .unwrap_or(NetworkProfile::LocalNetwork))
    }

    /// Persist the network profile used the next time the engine starts.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on write failure.
    pub fn set_network_profile(&mut self, profile: NetworkProfile) -> Result<(), LibraryError> {
        Ok(self
            .storage
            .set_setting(NETWORK_PROFILE_KEY, profile.name())?)
    }

    /// Configured peer port and router forwarding for shared courses; an
    /// ephemeral port without forwarding unless the learner chose otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn peer_reachability(&self) -> Result<PeerReachability, LibraryError> {
        Ok(PeerReachability::from_settings(
            self.storage.setting(PEER_PORT_KEY)?.as_deref(),
            self.storage.setting(PORT_FORWARDING_KEY)?.as_deref(),
        ))
    }

    /// Persist the peer port and forwarding used the next time the engines start.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on write failure.
    pub fn set_peer_reachability(
        &mut self,
        reachability: PeerReachability,
    ) -> Result<(), LibraryError> {
        self.storage
            .set_setting(PEER_PORT_KEY, &reachability.port_setting())?;
        Ok(self
            .storage
            .set_setting(PORT_FORWARDING_KEY, reachability.port_forwarding_setting())?)
    }

    /// Stored speed caps for both engines; unlimited unless the learner chose.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn rate_limits(&self) -> Result<RateLimits, LibraryError> {
        Ok(RateLimits::from_settings(
            self.storage.setting(DOWNLOAD_LIMIT_KEY)?.as_deref(),
            self.storage.setting(UPLOAD_LIMIT_KEY)?.as_deref(),
        ))
    }

    /// Persist speed caps.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on write failure.
    pub fn set_rate_limits(&mut self, limits: RateLimits) -> Result<(), LibraryError> {
        let (download, upload) = limits.to_settings();
        self.storage.set_setting(DOWNLOAD_LIMIT_KEY, &download)?;
        Ok(self.storage.set_setting(UPLOAD_LIMIT_KEY, &upload)?)
    }

    /// The stored torrent source of one course.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn torrent_source(
        &self,
        course_id: &str,
    ) -> Result<Option<TorrentCourseSource>, LibraryError> {
        Ok(self
            .storage
            .torrent_course_sources()?
            .into_iter()
            .find(|source| source.course_id == course_id))
    }

    /// Every stored torrent course source.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn torrent_sources(&self) -> Result<Vec<TorrentCourseSource>, LibraryError> {
        Ok(self.storage.torrent_course_sources()?)
    }

    /// When each lesson resource of a course was last played, in Unix seconds.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn playback_recency(
        &self,
        course_id: &str,
    ) -> Result<std::collections::HashMap<String, u64>, LibraryError> {
        Ok(self.storage.playback_recency(course_id)?)
    }

    /// The cap on streamed lessons that are not kept offline, in bytes.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn cache_limit(&self) -> Result<Option<u64>, LibraryError> {
        Ok(self
            .storage
            .setting(CACHE_LIMIT_KEY)?
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|&bytes| bytes > 0))
    }

    /// Persist the cap on streamed lessons; `None` removes it.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on write failure.
    pub fn set_cache_limit(&mut self, bytes: Option<u64>) -> Result<(), LibraryError> {
        let value = bytes
            .filter(|&bytes| bytes > 0)
            .map_or_else(String::new, |bytes| bytes.to_string());
        Ok(self.storage.set_setting(CACHE_LIMIT_KEY, &value)?)
    }

    /// List durably imported courses.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query or decoding failure.
    pub fn courses(&self) -> Result<Vec<StoredCourseSummary>, LibraryError> {
        self.storage.list_courses().map_err(LibraryError::from)
    }

    /// Search durably imported course metadata using FTS5 syntax.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] when the query is invalid or storage
    /// fails.
    pub fn search(&self, query: &str) -> Result<Vec<StoredCourseSummary>, LibraryError> {
        self.storage
            .search_courses(query)
            .map_err(LibraryError::from)
    }

    /// Remember where the learner stopped in a lesson resource. A position in
    /// the last few seconds counts as finished and is stored as the start.
    /// Returns `false` when the course has no such resource.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::InvalidPlaybackTime`] for negative, infinite,
    /// or out-of-range values, or a storage failure.
    pub fn save_playback_position(
        &mut self,
        course_id: &str,
        resource_id: &str,
        position_seconds: f64,
        duration_seconds: f64,
    ) -> Result<bool, LibraryError> {
        if !duration_seconds.is_finite()
            || duration_seconds <= 0.0
            || duration_seconds > MAX_LESSON_SECONDS
        {
            return Err(LibraryError::InvalidPlaybackTime("duration"));
        }
        if !position_seconds.is_finite()
            || position_seconds < 0.0
            || position_seconds > duration_seconds + 1.0
        {
            return Err(LibraryError::InvalidPlaybackTime("position"));
        }
        if !self.has_resource(course_id, resource_id)? {
            return Ok(false);
        }
        self.complete_if_watched(course_id, resource_id, position_seconds, duration_seconds)?;
        let position = if duration_seconds - position_seconds <= FINISHED_WITHIN_SECONDS {
            0.0
        } else {
            position_seconds
        };
        self.storage.set_playback_position(
            course_id,
            resource_id,
            PlaybackPosition {
                position_ms: milliseconds(position),
                duration_ms: milliseconds(duration_seconds),
            },
        )?;
        Ok(true)
    }

    /// Where the learner stopped in a lesson resource, in seconds.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn playback_position(
        &self,
        course_id: &str,
        resource_id: &str,
    ) -> Result<Option<f64>, LibraryError> {
        Ok(self
            .storage
            .playback_position(course_id, resource_id)?
            .map(|position| Duration::from_millis(position.position_ms).as_secs_f64()))
    }

    fn has_resource(&self, course_id: &str, resource_id: &str) -> Result<bool, LibraryError> {
        Ok(self
            .storage
            .course_manifest(course_id)?
            .is_some_and(|manifest| {
                manifest
                    .modules
                    .iter()
                    .flat_map(|module| &module.lessons)
                    .flat_map(|lesson| &lesson.resources)
                    .any(|resource| resource.id == resource_id)
            }))
    }

    /// Load the validated manifest snapshot for a logical course.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query, decoding, or validation
    /// failure.
    pub fn manifest(&self, course_id: &str) -> Result<Option<CourseManifest>, LibraryError> {
        self.storage
            .course_manifest(course_id)
            .map_err(LibraryError::from)
    }

    /// Resolve a verified resource to a backend-only local path.
    ///
    /// This method is for media registration inside the Rust process. Its
    /// result must never be serialized over IPC.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] if the stored manifest or package
    /// location cannot be recovered.
    pub fn resource_path(
        &self,
        course_id: &str,
        resource_id: &str,
    ) -> Result<Option<std::path::PathBuf>, LibraryError> {
        if self.storage.course_torrent_info_hash(course_id)?.is_some() {
            return Ok(None);
        }
        let Some(manifest) = self.storage.course_manifest(course_id)? else {
            return Ok(None);
        };
        let Some(root) = self.storage.course_package_root(course_id)? else {
            return Ok(None);
        };
        let resource = manifest
            .modules
            .iter()
            .flat_map(|module| &module.lessons)
            .flat_map(|lesson| &lesson.resources)
            .find(|resource| resource.id == resource_id);
        Ok(resource.map(|resource| root.join(&resource.path)))
    }

    /// Resolve a resource and its delivery metadata for a backend adapter.
    ///
    /// The returned path must remain inside the Rust process.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] if the stored package cannot be recovered.
    pub fn local_resource(
        &self,
        course_id: &str,
        resource_id: &str,
    ) -> Result<Option<LocalResource>, LibraryError> {
        if self.storage.course_torrent_info_hash(course_id)?.is_some() {
            return Ok(None);
        }
        let Some(manifest) = self.storage.course_manifest(course_id)? else {
            return Ok(None);
        };
        let Some(root) = self.storage.course_package_root(course_id)? else {
            return Ok(None);
        };
        Ok(manifest
            .modules
            .iter()
            .flat_map(|module| &module.lessons)
            .flat_map(|lesson| &lesson.resources)
            .find(|resource| resource.id == resource_id)
            .map(|resource| LocalResource {
                path: root.join(&resource.path),
                media_type: resource.media_type.clone(),
                byte_length: resource.byte_length,
            }))
    }

    /// Every resource of a torrent-sourced course with its torrent file.
    /// Empty for local courses. Resources whose path or length disagrees with
    /// the stored torrent are left out; import already rejects such courses.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query or decoding failure.
    pub fn torrent_resources(&self, course_id: &str) -> Result<Vec<TorrentResource>, LibraryError> {
        let Some(source) = self
            .storage
            .torrent_course_sources()?
            .into_iter()
            .find(|source| source.course_id == course_id)
        else {
            return Ok(Vec::new());
        };
        let Some(manifest) = self.storage.course_manifest(course_id)? else {
            return Ok(Vec::new());
        };
        let Ok(files) = torrent_metainfo::file_table(&source.metainfo) else {
            return Ok(Vec::new());
        };
        Ok(manifest
            .modules
            .iter()
            .flat_map(|module| &module.lessons)
            .flat_map(|lesson| &lesson.resources)
            .filter_map(|resource| {
                let file_index = files.iter().position(|file| {
                    file.path == resource.path && file.length == resource.byte_length
                })?;
                Some(TorrentResource {
                    resource_id: resource.id.clone(),
                    info_hash: source.info_hash.clone(),
                    file_index,
                    media_type: resource.media_type.clone(),
                    byte_length: resource.byte_length,
                })
            })
            .collect())
    }
}

/// Whole milliseconds of a validated, non-negative, bounded number of seconds.
fn milliseconds(seconds: f64) -> u64 {
    u64::try_from(Duration::from_secs_f64(seconds).as_millis()).unwrap_or(u64::MAX)
}
