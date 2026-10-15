use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use thiserror::Error;
use torrent_port::{
    EngineError, FetchRequest, InfoHash, ResolveRequest, ResolvedTorrent, SeedRequest,
    TorrentEngine, TransferStats, VerifiedFile,
};

use crate::{CourseLibrary, LibraryError};

#[derive(Default)]
struct Routes {
    shared: HashSet<InfoHash>,
    /// The latest fetch per torrent, replayed when a transfer changes engine.
    requests: HashMap<InfoHash, FetchRequest>,
    /// Torrents the learner paused, paused again after a change of engine.
    paused: HashSet<InfoHash>,
}

/// Routes each torrent to a private engine that never uploads or, for courses
/// the learner shares, to a sharing engine. Engines that disable uploads do so
/// for their whole session, so per-course sharing needs two sessions.
pub struct TransferRouter<E> {
    private: Arc<E>,
    sharing: Option<Arc<E>>,
    routes: Mutex<Routes>,
    /// Serializes fetches, stops, and moves so a transfer is never in both engines.
    changes: tokio::sync::Mutex<()>,
}

impl<E: TorrentEngine> TransferRouter<E> {
    /// Compose the router. Without a sharing engine, every torrent stays private.
    #[must_use]
    pub fn new(private: Arc<E>, sharing: Option<Arc<E>>) -> Self {
        Self {
            private,
            sharing,
            routes: Mutex::new(Routes::default()),
            changes: tokio::sync::Mutex::new(()),
        }
    }

    /// Whether courses can be shared at all.
    #[must_use]
    pub fn sharing_available(&self) -> bool {
        self.sharing.is_some()
    }

    /// Whether a torrent is routed to the sharing engine.
    #[must_use]
    pub fn is_shared(&self, info_hash: &InfoHash) -> bool {
        self.sharing.is_some() && self.routes().shared.contains(info_hash)
    }

    /// Record stored choices before any transfer starts.
    pub fn prefer_shared(&self, info_hash: InfoHash) {
        self.routes().shared.insert(info_hash);
    }

    /// Route a torrent to the sharing or private engine. An active transfer is
    /// stopped and started again in the other engine with the same selection;
    /// that engine re-checks the data already on disk instead of downloading it.
    ///
    /// # Errors
    /// [`EngineError::UnsupportedCapability`] when sharing is unavailable, or
    /// the engine failure that prevented the move. A failed move is undone.
    pub async fn set_shared(&self, info_hash: &InfoHash, shared: bool) -> Result<(), EngineError> {
        if shared && self.sharing.is_none() {
            return Err(EngineError::UnsupportedCapability("sharing"));
        }
        let _changes = self.changes.lock().await;
        let was_shared = self.is_shared(info_hash);
        if was_shared == shared {
            return Ok(());
        }
        let request = self.routes().requests.get(info_hash).cloned();
        let from = Arc::clone(self.engine(was_shared));
        let to = Arc::clone(self.engine(shared));
        self.mark(info_hash, shared);
        let (Some(request), true) = (request, from.transfer(info_hash).is_some()) else {
            return Ok(());
        };
        match from.stop(info_hash).await {
            Ok(()) | Err(EngineError::NotActive) => {}
            Err(error) => {
                self.mark(info_hash, was_shared);
                return Err(error);
            }
        }
        if let Err(error) = to.fetch(request.clone()).await {
            self.mark(info_hash, was_shared);
            from.fetch(request).await?;
            return Err(error);
        }
        if self.routes().paused.contains(info_hash) {
            to.pause(info_hash).await?;
        }
        Ok(())
    }

    fn mark(&self, info_hash: &InfoHash, shared: bool) {
        let mut routes = self.routes();
        if shared {
            routes.shared.insert(info_hash.clone());
        } else {
            routes.shared.remove(info_hash);
        }
    }

    fn engine(&self, shared: bool) -> &Arc<E> {
        match (&self.sharing, shared) {
            (Some(sharing), true) => sharing,
            _ => &self.private,
        }
    }

    fn engine_for(&self, info_hash: &InfoHash) -> &Arc<E> {
        self.engine(self.is_shared(info_hash))
    }

    fn routes(&self) -> MutexGuard<'_, Routes> {
        self.routes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<E: TorrentEngine> TorrentEngine for TransferRouter<E> {
    async fn seed(&self, request: SeedRequest) -> Result<InfoHash, EngineError> {
        match &self.sharing {
            Some(sharing) => sharing.seed(request).await,
            None => Err(EngineError::UnsupportedCapability("sharing")),
        }
    }

    async fn resolve(&self, request: ResolveRequest) -> Result<ResolvedTorrent, EngineError> {
        self.private.resolve(request).await
    }

    async fn fetch(&self, request: FetchRequest) -> Result<InfoHash, EngineError> {
        let info_hash = torrent_metainfo::info_hash(&request.metainfo)
            .map_err(|error| EngineError::InvalidTorrent(error.to_string()))
            .and_then(|hash| InfoHash::parse(&hash))?;
        let _changes = self.changes.lock().await;
        let engine = Arc::clone(self.engine_for(&info_hash));
        let fetched = engine.fetch(request.clone()).await?;
        self.routes().requests.insert(info_hash, request);
        Ok(fetched)
    }

    /// Also updates the remembered request, so moving the torrent between
    /// engines when sharing changes keeps the selection.
    async fn select_files(&self, info_hash: &InfoHash, files: &[usize]) -> Result<(), EngineError> {
        let _changes = self.changes.lock().await;
        let engine = Arc::clone(self.engine_for(info_hash));
        engine.select_files(info_hash, files).await?;
        if let Some(request) = self.routes().requests.get_mut(info_hash) {
            request.files = files.to_vec();
        }
        Ok(())
    }

    async fn pause(&self, info_hash: &InfoHash) -> Result<(), EngineError> {
        let _changes = self.changes.lock().await;
        let engine = Arc::clone(self.engine_for(info_hash));
        engine.pause(info_hash).await?;
        self.routes().paused.insert(info_hash.clone());
        Ok(())
    }

    async fn resume(&self, info_hash: &InfoHash) -> Result<(), EngineError> {
        let _changes = self.changes.lock().await;
        let engine = Arc::clone(self.engine_for(info_hash));
        engine.resume(info_hash).await?;
        self.routes().paused.remove(info_hash);
        Ok(())
    }

    async fn stop(&self, info_hash: &InfoHash) -> Result<(), EngineError> {
        let _changes = self.changes.lock().await;
        self.routes().requests.remove(info_hash);
        self.routes().paused.remove(info_hash);
        self.engine_for(info_hash).stop(info_hash).await
    }

    async fn open_file(
        &self,
        info_hash: &InfoHash,
        file_index: usize,
    ) -> Result<Box<dyn VerifiedFile>, EngineError> {
        let engine = Arc::clone(self.engine_for(info_hash));
        engine.open_file(info_hash, file_index).await
    }

    fn transfer(&self, info_hash: &InfoHash) -> Option<TransferStats> {
        self.engine_for(info_hash).transfer(info_hash)
    }

    fn transfers(&self) -> Vec<TransferStats> {
        let mut transfers = self.private.transfers();
        if let Some(sharing) = &self.sharing {
            transfers.extend(sharing.transfers());
        }
        transfers.sort_by(|left, right| left.info_hash.cmp(&right.info_hash));
        transfers
    }

    fn listen_address(&self) -> Option<SocketAddr> {
        self.private.listen_address()
    }
}

/// Sharing state and engine-local metrics of one torrent-sourced course.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseSharingStatus {
    /// Stable logical course ID.
    pub course_id: String,
    /// The learner's stored choice.
    pub sharing: bool,
    /// Current transfer, measured by this computer in this session only.
    pub transfer: Option<TransferStats>,
}

/// Why a sharing choice could not be applied.
#[derive(Debug, Error)]
pub enum SharingError {
    /// The course does not exist or was not imported from a torrent.
    #[error("only courses imported from a torrent can be shared")]
    NotTorrentCourse,
    /// The sharing engine is not running.
    #[error("sharing is unavailable because its torrent engine could not start")]
    Unavailable,
    /// The library could not be read or written.
    #[error(transparent)]
    Library(#[from] LibraryError),
    /// The engine could not move the transfer.
    #[error(transparent)]
    Engine(#[from] EngineError),
}

/// Learner choices about which torrent-sourced courses to share.
pub struct CourseSharing<E> {
    router: Arc<TransferRouter<E>>,
    library: Arc<Mutex<CourseLibrary>>,
}

impl<E: TorrentEngine> CourseSharing<E> {
    /// Compose the use case over the router that also carries imports.
    #[must_use]
    pub fn new(router: Arc<TransferRouter<E>>, library: Arc<Mutex<CourseLibrary>>) -> Self {
        Self { router, library }
    }

    /// Apply stored choices; call before transfers are restored.
    ///
    /// # Errors
    /// Returns a library failure.
    pub fn load(&self) -> Result<(), LibraryError> {
        for source in self.library().torrent_course_sources()? {
            if source.sharing
                && let Ok(info_hash) = InfoHash::parse(&source.info_hash)
            {
                self.router.prefer_shared(info_hash);
            }
        }
        Ok(())
    }

    /// Whether courses can be shared at all.
    #[must_use]
    pub fn available(&self) -> bool {
        self.router.sharing_available()
    }

    /// Share or stop sharing a course. The choice is stored only after the
    /// transfer is in the matching engine; turning sharing off always succeeds
    /// in storage even if the sharing engine is gone.
    ///
    /// # Errors
    /// See [`SharingError`].
    pub async fn set(&self, course_id: &str, sharing: bool) -> Result<(), SharingError> {
        let info_hash = self
            .library()
            .torrent_info_hash(course_id)?
            .ok_or(SharingError::NotTorrentCourse)?;
        let info_hash = InfoHash::parse(&info_hash)?;
        if sharing && !self.router.sharing_available() {
            return Err(SharingError::Unavailable);
        }
        self.router.set_shared(&info_hash, sharing).await?;
        let stored = self.library().set_course_sharing(course_id, sharing);
        if let Err(error) = stored {
            // Keep the engine consistent with what is stored.
            let _ = self.router.set_shared(&info_hash, !sharing).await;
            return Err(error.into());
        }
        Ok(())
    }

    /// Every torrent-sourced course with its choice and local metrics.
    ///
    /// # Errors
    /// Returns a library failure.
    pub fn statuses(&self) -> Result<Vec<CourseSharingStatus>, LibraryError> {
        let sources = self.library().torrent_course_sources()?;
        Ok(sources
            .into_iter()
            .map(|source| CourseSharingStatus {
                transfer: InfoHash::parse(&source.info_hash)
                    .ok()
                    .and_then(|info_hash| self.router.transfer(&info_hash)),
                course_id: source.course_id,
                sharing: source.sharing,
            })
            .collect())
    }

    fn library(&self) -> MutexGuard<'_, CourseLibrary> {
        self.library.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
