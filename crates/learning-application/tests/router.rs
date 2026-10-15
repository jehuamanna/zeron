//! Routing file selection between the private and sharing engines.

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use learning_application::{EngineError, InfoHash, TorrentEngine, TransferRouter};
use torrent_metainfo::{Value, encode, info_hash};
use torrent_port::{
    FetchRequest, ResolveRequest, ResolvedTorrent, SeedRequest, TransferState, TransferStats,
    VerifiedFile,
};

/// Records fetches and selections; a fetched torrent stays active until stopped.
#[derive(Default)]
struct RecordingEngine {
    active: Mutex<Vec<FetchRequest>>,
    selections: Mutex<Vec<Vec<usize>>>,
    paused: Mutex<bool>,
}

#[allow(
    unknown_lints,
    clippy::unused_async_trait_impl,
    reason = "fakes mirror the async engine contract"
)]
impl TorrentEngine for RecordingEngine {
    async fn seed(&self, _: SeedRequest) -> Result<InfoHash, EngineError> {
        Err(EngineError::UnsupportedCapability("seed"))
    }

    async fn resolve(&self, _: ResolveRequest) -> Result<ResolvedTorrent, EngineError> {
        Err(EngineError::UnsupportedCapability("resolve"))
    }

    async fn fetch(&self, request: FetchRequest) -> Result<InfoHash, EngineError> {
        let hash = InfoHash::parse(&info_hash(&request.metainfo).unwrap())?;
        self.active.lock().unwrap().push(request);
        Ok(hash)
    }

    async fn select_files(&self, _: &InfoHash, files: &[usize]) -> Result<(), EngineError> {
        if self.active.lock().unwrap().is_empty() {
            return Err(EngineError::NotActive);
        }
        self.selections.lock().unwrap().push(files.to_vec());
        Ok(())
    }

    async fn pause(&self, _: &InfoHash) -> Result<(), EngineError> {
        if self.active.lock().unwrap().is_empty() {
            return Err(EngineError::NotActive);
        }
        *self.paused.lock().unwrap() = true;
        Ok(())
    }

    async fn resume(&self, _: &InfoHash) -> Result<(), EngineError> {
        *self.paused.lock().unwrap() = false;
        Ok(())
    }

    async fn stop(&self, _: &InfoHash) -> Result<(), EngineError> {
        self.active.lock().unwrap().clear();
        *self.paused.lock().unwrap() = false;
        Ok(())
    }

    async fn open_file(
        &self,
        _: &InfoHash,
        _: usize,
    ) -> Result<Box<dyn VerifiedFile>, EngineError> {
        Err(EngineError::UnsupportedCapability("streaming"))
    }

    fn transfer(&self, info_hash: &InfoHash) -> Option<TransferStats> {
        (!self.active.lock().unwrap().is_empty()).then(|| TransferStats {
            info_hash: info_hash.clone(),
            state: TransferState::Downloading,
            verified_bytes: 0,
            total_bytes: 1,
            uploaded_bytes: 0,
            upload_bytes_per_second: 0,
            downloaded_bytes: 0,
            connected_peers: 0,
            file_verified_bytes: vec![0],
            failure: None,
        })
    }

    fn transfers(&self) -> Vec<TransferStats> {
        Vec::new()
    }

    fn listen_address(&self) -> Option<SocketAddr> {
        None
    }
}

fn metainfo() -> Vec<u8> {
    let info = Value::Dict(BTreeMap::from([(
        b"name".to_vec(),
        Value::Bytes(b"course".to_vec()),
    )]));
    encode(&Value::Dict(BTreeMap::from([(b"info".to_vec(), info)])))
}

#[tokio::test]
async fn a_selection_follows_the_torrent_when_sharing_changes() {
    let private = Arc::new(RecordingEngine::default());
    let sharing = Arc::new(RecordingEngine::default());
    let router = TransferRouter::new(Arc::clone(&private), Some(Arc::clone(&sharing)));
    let hash = router
        .fetch(FetchRequest {
            metainfo: metainfo(),
            content_root: PathBuf::from("/content"),
            files: vec![0],
            initial_peers: vec![],
        })
        .await
        .unwrap();

    router.select_files(&hash, &[0, 3, 4]).await.unwrap();
    assert_eq!(*private.selections.lock().unwrap(), [vec![0, 3, 4]]);
    router.set_shared(&hash, true).await.unwrap();
    assert!(private.active.lock().unwrap().is_empty());
    assert_eq!(sharing.active.lock().unwrap()[0].files, [0, 3, 4]);

    router.select_files(&hash, &[0]).await.unwrap();
    assert_eq!(*sharing.selections.lock().unwrap(), [vec![0]]);
    router.set_shared(&hash, false).await.unwrap();
    assert_eq!(private.active.lock().unwrap()[0].files, [0]);

    let unknown = InfoHash::parse(&"e".repeat(40)).unwrap();
    router.stop(&hash).await.unwrap();
    assert!(matches!(
        router.select_files(&unknown, &[0]).await,
        Err(EngineError::NotActive)
    ));
}

#[tokio::test]
async fn a_pause_follows_the_torrent_when_sharing_changes() {
    let private = Arc::new(RecordingEngine::default());
    let sharing = Arc::new(RecordingEngine::default());
    let router = TransferRouter::new(Arc::clone(&private), Some(Arc::clone(&sharing)));
    let hash = router
        .fetch(FetchRequest {
            metainfo: metainfo(),
            content_root: PathBuf::from("/content"),
            files: vec![0],
            initial_peers: vec![],
        })
        .await
        .unwrap();
    router.pause(&hash).await.unwrap();
    assert!(*private.paused.lock().unwrap());
    router.set_shared(&hash, true).await.unwrap();
    assert!(
        *sharing.paused.lock().unwrap(),
        "paused again on the new engine"
    );
    router.resume(&hash).await.unwrap();
    assert!(!*sharing.paused.lock().unwrap());
    router.set_shared(&hash, false).await.unwrap();
    assert!(
        !*private.paused.lock().unwrap(),
        "a resumed torrent stays running"
    );
}
