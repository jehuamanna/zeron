//! Free disk space checks, so downloads stop before the disk fills up.

use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use torrent_port::{EngineError, InfoHash, TorrentEngine};

use crate::CourseLibrary;

/// Free space downloads must leave on the learner's disk.
pub const DISK_RESERVE_BYTES: u64 = 1024 * 1024 * 1024;

/// Bytes available to this user on the filesystem holding `path`, or `None`
/// where the platform check is unavailable.
#[must_use]
pub fn available_bytes(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    #[allow(
        clippy::useless_conversion,
        reason = "the field types differ between platforms"
    )]
    {
        let stats = nix::sys::statvfs::statvfs(path).ok()?;
        u64::from(stats.blocks_available()).checked_mul(u64::from(stats.fragment_size()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// Whether `needed` more bytes fit while keeping [`DISK_RESERVE_BYTES`] free.
/// Unknown free space never refuses.
#[must_use]
pub fn fits(available: Option<u64>, needed: u64) -> bool {
    available.is_none_or(|available| available >= needed.saturating_add(DISK_RESERVE_BYTES))
}

/// Disk state as last checked by [`DiskGuard::check`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiskState {
    /// Free bytes, when the platform can tell.
    pub available_bytes: Option<u64>,
    /// Downloads are paused because free space fell below the reserve.
    pub low: bool,
}

/// Pauses running downloads when free space falls below the reserve and
/// resumes the ones it paused once space recovers. Courses the learner
/// paused are never resumed by it.
pub struct DiskGuard<E> {
    engine: Arc<E>,
    library: Arc<Mutex<CourseLibrary>>,
    root: std::path::PathBuf,
    paused: Mutex<Vec<InfoHash>>,
    state: Mutex<DiskState>,
}

impl<E: TorrentEngine> DiskGuard<E> {
    /// Guard the filesystem that holds `root`, the learner's downloads.
    #[must_use]
    pub fn new(
        engine: Arc<E>,
        library: Arc<Mutex<CourseLibrary>>,
        root: std::path::PathBuf,
    ) -> Self {
        Self {
            engine,
            library,
            root,
            paused: Mutex::new(Vec::new()),
            state: Mutex::new(DiskState::default()),
        }
    }

    /// The state found by the latest check.
    #[must_use]
    pub fn state(&self) -> DiskState {
        *lock(&self.state)
    }

    /// Free bytes on the guarded filesystem right now, when known.
    #[must_use]
    pub fn measure(&self) -> Option<u64> {
        available_bytes(existing_ancestor(&self.root))
    }

    /// Measure free space and pause or resume downloads accordingly.
    pub async fn check(&self) -> DiskState {
        let available = self.measure();
        self.apply(available).await
    }

    /// Apply a measurement: below the reserve, pause every active course the
    /// learner has not paused; above one and a half reserves, resume them.
    pub async fn apply(&self, available: Option<u64>) -> DiskState {
        let was_low = self.state().low;
        let low = available.is_some_and(|available| {
            if was_low {
                available < DISK_RESERVE_BYTES + DISK_RESERVE_BYTES / 2
            } else {
                available < DISK_RESERVE_BYTES
            }
        });
        if low && !was_low {
            for info_hash in self.running_courses() {
                if self.engine.pause(&info_hash).await.is_ok() {
                    lock(&self.paused).push(info_hash);
                }
            }
        } else if !low && was_low {
            let paused = std::mem::take(&mut *lock(&self.paused));
            for info_hash in paused {
                match self.engine.resume(&info_hash).await {
                    Ok(()) | Err(EngineError::NotActive) => {}
                    Err(_) => lock(&self.paused).push(info_hash),
                }
            }
        }
        let state = DiskState {
            available_bytes: available,
            low,
        };
        *lock(&self.state) = state;
        state
    }

    fn running_courses(&self) -> Vec<InfoHash> {
        let sources = lock(&self.library).torrent_sources().unwrap_or_default();
        sources
            .into_iter()
            .filter(|source| !source.paused)
            .filter_map(|source| InfoHash::parse(&source.info_hash).ok())
            .filter(|info_hash| self.engine.transfer(info_hash).is_some())
            .collect()
    }
}

fn existing_ancestor(path: &Path) -> &Path {
    path.ancestors()
        .find(|candidate| candidate.exists())
        .unwrap_or(path)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
