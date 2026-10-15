//! Opening torrent lessons for playback.

use std::time::Duration;

use torrent_port::{EngineError, InfoHash, TorrentEngine, VerifiedFile};

/// How long a lesson waits for its course torrent to become active.
///
/// At startup each torrent course is restored in the background, and the
/// engine re-checks local pieces before the torrent is active. A lesson
/// opened in that window would otherwise fail at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamWait {
    /// Give up after this long and report the engine's error.
    pub limit: Duration,
    /// Delay between attempts.
    pub poll: Duration,
}

impl Default for StreamWait {
    fn default() -> Self {
        Self {
            limit: Duration::from_secs(30),
            poll: Duration::from_millis(200),
        }
    }
}

/// Open a verified reader for one torrent file, waiting up to `wait.limit`
/// while the engine reports the torrent as not active yet. Every other
/// failure is returned immediately.
///
/// # Errors
/// Returns the engine's error, including [`EngineError::NotActive`] when the
/// torrent did not become active in time.
pub async fn open_lesson_stream<E: TorrentEngine + ?Sized>(
    engine: &E,
    info_hash: &InfoHash,
    file_index: usize,
    wait: StreamWait,
) -> Result<Box<dyn VerifiedFile>, EngineError> {
    let deadline = tokio::time::Instant::now() + wait.limit;
    loop {
        match engine.open_file(info_hash, file_index).await {
            Err(EngineError::NotActive) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(wait.poll).await;
            }
            result => return result,
        }
    }
}
