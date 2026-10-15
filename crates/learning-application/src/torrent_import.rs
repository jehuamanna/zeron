use std::{
    collections::{BTreeSet, HashMap},
    fmt::Write as _,
    future::Future,
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use course_format::{CourseManifest, parse_and_validate};
use thiserror::Error;
use torrent_port::{
    EngineError, FetchRequest, InfoHash, ResolveRequest, ResolvedTorrent, TorrentEngine,
    TorrentSource, TransferState,
};

use crate::{CancellationFlag, CourseLibrary, ImportLimits};

const MANIFEST_PATH: &str = "course.json";
const RETAINED_FINISHED_IMPORTS: usize = 16;

/// Stage of a metadata-first torrent import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TorrentImportStage {
    /// Registered, not yet started.
    Queued,
    /// Waiting for verified torrent metadata.
    ResolvingMetadata,
    /// Downloading only `course.json`.
    FetchingManifest,
    /// Validating the manifest against the torrent inventory.
    Validating,
    /// The course is in the library.
    Imported,
    /// Import failed with a retryable reason; nothing was added.
    Failed,
    /// The learner cancelled; nothing was added.
    Cancelled,
}

impl TorrentImportStage {
    /// Whether the import can no longer change.
    #[must_use]
    pub const fn is_finished(self) -> bool {
        matches!(self, Self::Imported | Self::Failed | Self::Cancelled)
    }
}

/// Path-free failure classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentImportFailure {
    /// Stable machine-readable code.
    pub code: &'static str,
    /// Human-readable, actionable message.
    pub message: String,
}

/// Point-in-time import state. The snapshot is authoritative; notifications
/// carry the same value as hints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentImportSnapshot {
    /// Opaque job ID.
    pub job_id: String,
    /// Monotonic version.
    pub sequence: u64,
    /// Current stage.
    pub stage: TorrentImportStage,
    /// Info hash once metadata is verified.
    pub info_hash: Option<String>,
    /// Verified and total bytes of `course.json` while fetching it.
    pub manifest_bytes: Option<(u64, u64)>,
    /// Imported course ID.
    pub course_id: Option<String>,
    /// Imported course title.
    pub title: Option<String>,
    /// Failure details.
    pub failure: Option<TorrentImportFailure>,
}

/// Bounds on waiting for peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TorrentImportTimeouts {
    /// Longest wait for metadata.
    pub metadata: Duration,
    /// Longest period without `course.json` progress.
    pub stall: Duration,
    /// Progress and cancellation polling interval.
    pub poll: Duration,
}

impl Default for TorrentImportTimeouts {
    fn default() -> Self {
        Self {
            metadata: Duration::from_secs(120),
            stall: Duration::from_secs(60),
            poll: Duration::from_millis(100),
        }
    }
}

/// Another import is running.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
#[error("another course import is already running")]
pub struct TorrentImportBusy;

struct Entry {
    snapshot: TorrentImportSnapshot,
    cancellation: CancellationFlag,
    finished_order: Option<u64>,
}

#[derive(Default)]
struct Registry {
    jobs: HashMap<String, Entry>,
    active: Option<String>,
    finished_counter: u64,
}

enum Stop {
    Failed(TorrentImportFailure),
    Cancelled,
}

impl From<TorrentImportFailure> for Stop {
    fn from(failure: TorrentImportFailure) -> Self {
        Self::Failed(failure)
    }
}

/// Imports courses from `.torrent` files and magnet links, fetching only
/// `course.json` and inventing no structure before it validates.
pub struct TorrentImports<E> {
    engine: Arc<E>,
    library: Arc<Mutex<CourseLibrary>>,
    courses_root: PathBuf,
    limits: ImportLimits,
    timeouts: TorrentImportTimeouts,
    registry: Mutex<Registry>,
}

impl<E: TorrentEngine> TorrentImports<E> {
    /// Compose the import service. Downloads go below `courses_root/<info hash>/`.
    #[must_use]
    pub fn new(
        engine: Arc<E>,
        library: Arc<Mutex<CourseLibrary>>,
        courses_root: PathBuf,
        limits: ImportLimits,
        timeouts: TorrentImportTimeouts,
    ) -> Self {
        Self {
            engine,
            library,
            courses_root,
            limits,
            timeouts,
            registry: Mutex::new(Registry::default()),
        }
    }

    /// Register an import. Only one import runs at a time.
    ///
    /// # Errors
    /// Returns [`TorrentImportBusy`] while another import is running.
    pub fn begin(&self) -> Result<TorrentImportSnapshot, TorrentImportBusy> {
        let mut registry = self.lock();
        if registry.active.is_some() {
            return Err(TorrentImportBusy);
        }
        let job_id = opaque_id();
        let snapshot = TorrentImportSnapshot {
            job_id: job_id.clone(),
            sequence: 0,
            stage: TorrentImportStage::Queued,
            info_hash: None,
            manifest_bytes: None,
            course_id: None,
            title: None,
            failure: None,
        };
        registry.jobs.insert(
            job_id.clone(),
            Entry {
                snapshot: snapshot.clone(),
                cancellation: CancellationFlag::default(),
                finished_order: None,
            },
        );
        registry.active = Some(job_id);
        Ok(snapshot)
    }

    /// Current snapshot of an import.
    #[must_use]
    pub fn snapshot(&self, job_id: &str) -> Option<TorrentImportSnapshot> {
        self.lock()
            .jobs
            .get(job_id)
            .map(|entry| entry.snapshot.clone())
    }

    /// Request cancellation. Returns `false` for unknown or finished imports.
    pub fn cancel(&self, job_id: &str) -> bool {
        self.lock().jobs.get(job_id).is_some_and(|entry| {
            let running = !entry.snapshot.stage.is_finished();
            if running {
                entry.cancellation.cancel();
            }
            running
        })
    }

    /// Run a registered import to completion, reporting every change.
    pub async fn run(
        &self,
        job_id: &str,
        source: TorrentSource,
        initial_peers: Vec<SocketAddr>,
        notify: &mut (dyn FnMut(&TorrentImportSnapshot) + Send),
    ) -> Option<TorrentImportSnapshot> {
        let cancellation = self.lock().jobs.get(job_id)?.cancellation.clone();
        let mut started: Option<(InfoHash, PathBuf, bool)> = None;
        let outcome = self
            .import(
                job_id,
                source,
                initial_peers,
                &cancellation,
                notify,
                &mut started,
            )
            .await;
        let finished = match outcome {
            Ok(manifest) => self.update(job_id, notify, |snapshot| {
                snapshot.stage = TorrentImportStage::Imported;
                snapshot.course_id = Some(manifest.course_id);
                snapshot.title = Some(manifest.title);
            }),
            Err(stop) => {
                if let Some((info_hash, content_root, created)) = started {
                    self.discard(&info_hash, &content_root, created).await;
                }
                self.update(job_id, notify, |snapshot| match stop {
                    Stop::Cancelled => snapshot.stage = TorrentImportStage::Cancelled,
                    Stop::Failed(failure) => {
                        snapshot.stage = TorrentImportStage::Failed;
                        snapshot.failure = Some(failure);
                    }
                })
            }
        };
        self.finish(job_id);
        finished
    }

    /// Re-activate every torrent-sourced course's manifest after a restart.
    /// Returns how many were restored; failures leave the library unchanged.
    pub async fn restore(&self) -> usize {
        let Ok(sources) = self.library().torrent_course_sources() else {
            return 0;
        };
        let mut restored = 0;
        for source in sources {
            let resolved = self
                .engine
                .resolve(ResolveRequest {
                    source: TorrentSource::Metainfo(source.metainfo.clone()),
                    initial_peers: Vec::new(),
                })
                .await;
            let Some(manifest) = resolved.ok().and_then(|resolved| {
                resolved
                    .files
                    .iter()
                    .find(|file| file.path == MANIFEST_PATH)
                    .map(|file| file.index)
            }) else {
                continue;
            };
            let fetched = self
                .engine
                .fetch(FetchRequest {
                    metainfo: source.metainfo,
                    content_root: source.content_root,
                    files: vec![manifest],
                    initial_peers: Vec::new(),
                })
                .await;
            if fetched.is_ok() {
                restored += 1;
            }
        }
        restored
    }

    async fn import(
        &self,
        job_id: &str,
        source: TorrentSource,
        initial_peers: Vec<SocketAddr>,
        cancellation: &CancellationFlag,
        notify: &mut (dyn FnMut(&TorrentImportSnapshot) + Send),
        started: &mut Option<(InfoHash, PathBuf, bool)>,
    ) -> Result<CourseManifest, Stop> {
        self.update(job_id, notify, |snapshot| {
            snapshot.stage = TorrentImportStage::ResolvingMetadata;
        });
        let resolved = self
            .interruptible(
                self.engine.resolve(ResolveRequest {
                    source,
                    initial_peers: initial_peers.clone(),
                }),
                cancellation,
                self.timeouts.metadata,
            )
            .await?
            .ok_or_else(|| {
                failure(
                    "metadata_timeout",
                    format!(
                        "No peer provided the course metadata within {} seconds. Check the link and your network, then retry.",
                        self.timeouts.metadata.as_secs()
                    ),
                )
            })?
            .map_err(engine_failure)?;
        let (manifest_index, manifest_length) = check_inventory(&resolved, self.limits)?;
        let info_hash = resolved.info_hash.clone();
        self.update(job_id, notify, |snapshot| {
            snapshot.stage = TorrentImportStage::FetchingManifest;
            snapshot.info_hash = Some(info_hash.to_string());
            snapshot.manifest_bytes = Some((0, manifest_length));
        });

        let content_root = self.courses_root.join(info_hash.as_str());
        let created = !content_root.exists();
        let fetched = self
            .engine
            .fetch(FetchRequest {
                metainfo: resolved.metainfo.clone(),
                content_root: content_root.clone(),
                files: vec![manifest_index],
                initial_peers: fetch_peers(initial_peers, &resolved.peers),
            })
            .await
            .map_err(engine_failure)?;
        *started = Some((fetched.clone(), content_root.clone(), created));
        self.wait_for_manifest(
            job_id,
            &fetched,
            manifest_index,
            manifest_length,
            cancellation,
            notify,
        )
        .await?;

        self.update(job_id, notify, |snapshot| {
            snapshot.stage = TorrentImportStage::Validating;
            snapshot.manifest_bytes = Some((manifest_length, manifest_length));
        });
        let manifest = read_manifest(&content_root, manifest_length)?;
        let total_resource_bytes = cross_check(&manifest, &resolved)?;
        if cancellation.is_cancelled() {
            return Err(Stop::Cancelled);
        }
        self.library()
            .save_torrent_course(
                &manifest,
                total_resource_bytes,
                &content_root,
                info_hash.as_str(),
                &resolved.metainfo,
            )
            .map_err(|error| failure("storage_failed", error.to_string()))?;
        Ok(manifest)
    }

    async fn wait_for_manifest(
        &self,
        job_id: &str,
        info_hash: &InfoHash,
        index: usize,
        length: u64,
        cancellation: &CancellationFlag,
        notify: &mut (dyn FnMut(&TorrentImportSnapshot) + Send),
    ) -> Result<(), Stop> {
        let mut last_verified = 0;
        let mut last_progress = tokio::time::Instant::now();
        loop {
            if cancellation.is_cancelled() {
                return Err(Stop::Cancelled);
            }
            let stats = self.engine.transfer(info_hash).ok_or_else(|| {
                failure(
                    "engine_failed",
                    "The download stopped unexpectedly.".to_owned(),
                )
            })?;
            if stats.state == TransferState::Failed {
                return Err(failure(
                    "engine_failed",
                    stats
                        .failure
                        .unwrap_or_else(|| "The download failed.".to_owned()),
                )
                .into());
            }
            let verified = stats.file_verified_bytes.get(index).copied().unwrap_or(0);
            if verified >= length {
                return Ok(());
            }
            if verified > last_verified {
                last_verified = verified;
                last_progress = tokio::time::Instant::now();
                self.update(job_id, notify, |snapshot| {
                    snapshot.manifest_bytes = Some((verified, length));
                });
            } else if last_progress.elapsed() >= self.timeouts.stall {
                return Err(failure(
                    "manifest_stalled",
                    format!(
                        "No peer shared course.json for {} seconds. Retry when a seeder is online.",
                        self.timeouts.stall.as_secs()
                    ),
                )
                .into());
            }
            tokio::time::sleep(self.timeouts.poll).await;
        }
    }

    /// Await `future` unless cancelled; `Ok(None)` means the timeout elapsed.
    async fn interruptible<T>(
        &self,
        future: impl Future<Output = T>,
        cancellation: &CancellationFlag,
        timeout: Duration,
    ) -> Result<Option<T>, Stop> {
        let deadline = tokio::time::Instant::now() + timeout;
        tokio::pin!(future);
        loop {
            tokio::select! {
                output = &mut future => return Ok(Some(output)),
                () = tokio::time::sleep(self.timeouts.poll) => {
                    if cancellation.is_cancelled() {
                        return Err(Stop::Cancelled);
                    }
                    if tokio::time::Instant::now() >= deadline {
                        return Ok(None);
                    }
                }
            }
        }
    }

    async fn discard(&self, info_hash: &InfoHash, content_root: &Path, created: bool) {
        let referenced = self
            .library()
            .torrent_course_sources()
            .is_ok_and(|sources| {
                sources
                    .iter()
                    .any(|source| source.info_hash == info_hash.as_str())
            });
        if referenced {
            return;
        }
        let _ = self.engine.stop(info_hash).await;
        if created {
            let _ = std::fs::remove_dir_all(content_root);
        }
    }

    fn update(
        &self,
        job_id: &str,
        notify: &mut (dyn FnMut(&TorrentImportSnapshot) + Send),
        change: impl FnOnce(&mut TorrentImportSnapshot),
    ) -> Option<TorrentImportSnapshot> {
        let snapshot = {
            let mut registry = self.lock();
            let entry = registry.jobs.get_mut(job_id)?;
            change(&mut entry.snapshot);
            entry.snapshot.sequence += 1;
            entry.snapshot.clone()
        };
        notify(&snapshot);
        Some(snapshot)
    }

    fn finish(&self, job_id: &str) {
        let mut registry = self.lock();
        registry.finished_counter += 1;
        let order = registry.finished_counter;
        if let Some(entry) = registry.jobs.get_mut(job_id) {
            entry.finished_order = Some(order);
        }
        if registry.active.as_deref() == Some(job_id) {
            registry.active = None;
        }
        let mut finished: Vec<(u64, String)> = registry
            .jobs
            .iter()
            .filter_map(|(id, entry)| entry.finished_order.map(|order| (order, id.clone())))
            .collect();
        if finished.len() > RETAINED_FINISHED_IMPORTS {
            finished.sort_unstable();
            let excess = finished.len() - RETAINED_FINISHED_IMPORTS;
            for (_, id) in finished.into_iter().take(excess) {
                registry.jobs.remove(&id);
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn library(&self) -> MutexGuard<'_, CourseLibrary> {
        self.library.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn check_inventory(
    resolved: &ResolvedTorrent,
    limits: ImportLimits,
) -> Result<(usize, u64), TorrentImportFailure> {
    if resolved.files.iter().any(|file| !safe_path(&file.path)) {
        return Err(failure(
            "unsafe_inventory",
            "The torrent contains an unsafe file path, so it was not imported.".to_owned(),
        ));
    }
    let mut manifests = resolved
        .files
        .iter()
        .filter(|file| file.path == MANIFEST_PATH);
    let (Some(manifest), None) = (manifests.next(), manifests.next()) else {
        return Err(failure(
            "not_a_course",
            "The torrent has no course.json at its root, so it is not a Zeron course.".to_owned(),
        ));
    };
    if manifest.length > limits.max_manifest_bytes {
        return Err(failure(
            "limit_exceeded",
            "The course.json in this torrent is larger than allowed.".to_owned(),
        ));
    }
    let total = resolved
        .files
        .iter()
        .try_fold(0_u64, |total, file| total.checked_add(file.length));
    if resolved.files.len() - 1 > limits.max_resource_count
        || total.is_none_or(|total| total > limits.max_total_resource_bytes)
    {
        return Err(failure(
            "limit_exceeded",
            "This course is larger than this library allows.".to_owned(),
        ));
    }
    Ok((manifest.index, manifest.length))
}

fn read_manifest(content_root: &Path, length: u64) -> Result<CourseManifest, TorrentImportFailure> {
    let bytes = std::fs::read(content_root.join(MANIFEST_PATH)).map_err(|_| {
        failure(
            "manifest_invalid",
            "The downloaded course.json could not be read.".to_owned(),
        )
    })?;
    if bytes.len() as u64 != length {
        return Err(failure(
            "manifest_invalid",
            "The downloaded course.json has an unexpected length.".to_owned(),
        ));
    }
    parse_and_validate(&bytes).map_err(|error| {
        failure(
            "manifest_invalid",
            format!("The course.json in this torrent is invalid: {error}."),
        )
    })
}

fn cross_check(
    manifest: &CourseManifest,
    resolved: &ResolvedTorrent,
) -> Result<u64, TorrentImportFailure> {
    let declared: BTreeSet<(&str, u64)> = manifest
        .modules
        .iter()
        .flat_map(|module| &module.lessons)
        .flat_map(|lesson| &lesson.resources)
        .map(|resource| (resource.path.as_str(), resource.byte_length))
        .collect();
    let inventory: BTreeSet<(&str, u64)> = resolved
        .files
        .iter()
        .filter(|file| file.path != MANIFEST_PATH)
        .map(|file| (file.path.as_str(), file.length))
        .collect();
    if declared != inventory {
        return Err(failure(
            "inventory_mismatch",
            "The course.json does not describe exactly the files in this torrent, so it was not imported."
                .to_owned(),
        ));
    }
    Ok(declared.iter().map(|(_, length)| length).sum())
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|segment| {
            let mut components = Path::new(segment).components();
            matches!(components.next(), Some(Component::Normal(_)))
                && components.next().is_none()
                && !segment.contains(['\\', ':', '\0'])
        })
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a map_err adapter"
)]
fn engine_failure(error: EngineError) -> Stop {
    let code = match error {
        EngineError::InvalidTorrent(_) => "invalid_source",
        _ => "engine_failed",
    };
    Stop::Failed(failure(code, error.to_string()))
}

fn failure(code: &'static str, message: String) -> TorrentImportFailure {
    TorrentImportFailure { code, message }
}

fn opaque_id() -> String {
    let mut bytes = [0_u8; 16];
    let _ = getrandom::fill(&mut bytes);
    let mut id = String::from("import_");
    for byte in bytes {
        let _ = write!(id, "{byte:02x}");
    }
    id
}

/// Caller-supplied peers first, then the peers that served the metadata.
fn fetch_peers(mut initial: Vec<SocketAddr>, resolved: &[SocketAddr]) -> Vec<SocketAddr> {
    for peer in resolved {
        if !initial.contains(peer) {
            initial.push(*peer);
        }
    }
    initial
}

#[cfg(test)]
mod tests {
    use super::{fetch_peers, safe_path};

    #[test]
    fn only_portable_relative_paths_are_safe() {
        assert!(safe_path("course.json"));
        assert!(safe_path("resources/a/b.mp4"));
        for unsafe_path in ["", "../x", "a//b", "/abs", "a\\b", "c:/x", "./x", "a/.."] {
            assert!(!safe_path(unsafe_path), "{unsafe_path}");
        }
    }

    #[test]
    fn metadata_peers_are_added_after_caller_peers_without_duplicates() {
        let caller = "127.0.0.1:7000".parse().unwrap();
        let seeder = "192.168.1.20:6881".parse().unwrap();
        assert_eq!(
            fetch_peers(vec![caller], &[seeder, caller]),
            [caller, seeder]
        );
        assert_eq!(fetch_peers(Vec::new(), &[]), []);
    }
}
