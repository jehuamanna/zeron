//! Lessons opened while their course torrent is still being restored.

use std::{
    net::SocketAddr,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use learning_application::{EngineError, InfoHash, StreamWait, TorrentEngine, open_lesson_stream};
use torrent_port::{
    FetchRequest, ResolveRequest, ResolvedTorrent, SeedRequest, TransferStats, VerifiedFile,
};

/// Reports `NotActive` for the first `inactive` opens, then serves a file or fails.
struct RestoringEngine {
    inactive: usize,
    then: Option<EngineError>,
    opens: AtomicUsize,
}

#[allow(
    unknown_lints,
    clippy::unused_async_trait_impl,
    reason = "fakes mirror the async engine contract"
)]
impl TorrentEngine for RestoringEngine {
    async fn seed(&self, _: SeedRequest) -> Result<InfoHash, EngineError> {
        Err(EngineError::UnsupportedCapability("seed"))
    }

    async fn resolve(&self, _: ResolveRequest) -> Result<ResolvedTorrent, EngineError> {
        Err(EngineError::UnsupportedCapability("resolve"))
    }

    async fn fetch(&self, _: FetchRequest) -> Result<InfoHash, EngineError> {
        Err(EngineError::UnsupportedCapability("fetch"))
    }

    async fn stop(&self, _: &InfoHash) -> Result<(), EngineError> {
        Ok(())
    }

    async fn open_file(
        &self,
        _: &InfoHash,
        _: usize,
    ) -> Result<Box<dyn VerifiedFile>, EngineError> {
        if self.opens.fetch_add(1, Ordering::SeqCst) < self.inactive {
            return Err(EngineError::NotActive);
        }
        match &self.then {
            Some(error) => Err(error.clone()),
            None => Ok(Box::new(std::io::Cursor::new(b"verified".to_vec()))),
        }
    }

    fn transfer(&self, _: &InfoHash) -> Option<TransferStats> {
        None
    }

    fn transfers(&self) -> Vec<TransferStats> {
        Vec::new()
    }

    fn listen_address(&self) -> Option<SocketAddr> {
        None
    }
}

fn engine(inactive: usize, then: Option<EngineError>) -> RestoringEngine {
    RestoringEngine {
        inactive,
        then,
        opens: AtomicUsize::new(0),
    }
}

fn hash() -> InfoHash {
    InfoHash::parse(&"a".repeat(40)).unwrap()
}

const QUICK: StreamWait = StreamWait {
    limit: Duration::from_secs(5),
    poll: Duration::from_millis(5),
};

#[tokio::test]
async fn a_lesson_opened_during_restore_waits_for_its_torrent() {
    let restoring = engine(3, None);
    let mut file = open_lesson_stream(&restoring, &hash(), 1, QUICK)
        .await
        .unwrap();
    let mut bytes = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut file, &mut bytes)
        .await
        .unwrap();
    assert_eq!(bytes, b"verified");
    assert_eq!(restoring.opens.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn other_failures_are_reported_at_once_and_the_wait_is_bounded() {
    let refusing = engine(
        0,
        Some(EngineError::Engine(
            "too many torrent files are open for reading".to_owned(),
        )),
    );
    assert!(matches!(
        open_lesson_stream(&refusing, &hash(), 0, QUICK).await,
        Err(EngineError::Engine(_))
    ));
    assert_eq!(refusing.opens.load(Ordering::SeqCst), 1);

    let missing = engine(usize::MAX, None);
    let started = std::time::Instant::now();
    let bounded = StreamWait {
        limit: Duration::from_millis(60),
        poll: Duration::from_millis(5),
    };
    assert!(matches!(
        open_lesson_stream(&missing, &hash(), 0, bounded).await,
        Err(EngineError::NotActive)
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(missing.opens.load(Ordering::SeqCst) > 1);
    assert_eq!(StreamWait::default().limit, Duration::from_secs(30));
}
