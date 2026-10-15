//! librqbit 9.0.1 adapter for the engine-neutral torrent port.
//!
//! Every network option is set explicitly from a [`NetworkProfile`]; librqbit's
//! own defaults (for example, DHT on) are never relied on. Seeded content is
//! served through a read-only storage adapter, so release files are never
//! opened for writing.

mod storage;

use std::{
    collections::{HashMap, HashSet},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerMode,
    ListenerOptions, ManagedTorrent, Session, SessionOptions, TorrentStatsState,
    api::TorrentIdOrHash, storage::StorageFactoryExt,
};
pub use torrent_port::PeerReachability;
use torrent_port::{
    EngineError, FetchRequest, InfoHash, NetworkProfile, ResolveRequest, ResolvedTorrent,
    SeedRequest, TorrentEngine, TorrentFile, TorrentSource, TransferState, TransferStats,
    VerifiedFile,
};

use crate::storage::{ContentMismatch, MetadataOnly, ReadOnlyContent};

const CHECK_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// How long a magnet resolve waits for its metadata peers to reconnect.
/// Must exceed local service discovery's one-second reply rate limit.
const PEER_HINT_WAIT: Duration = Duration::from_secs(2);
const CONTENT_CHANGED: &str =
    "the content on disk no longer matches the torrent; nothing is being served";

/// Engine construction parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    /// Which networks the engine may use.
    pub profile: NetworkProfile,
    /// Private directory for engine state. Seeding never writes content here.
    pub state_directory: PathBuf,
    /// Peer port and router forwarding; the profile decides which parts apply
    /// (see [`PeerReachability::for_profile`]).
    pub reachability: PeerReachability,
    /// Whether peers may download from this engine. When off, the engine
    /// advertises no pieces, rejects every piece request, and cannot seed.
    pub allow_uploads: bool,
}

struct Active {
    handle: Arc<ManagedTorrent>,
    failure: Arc<Mutex<Option<String>>>,
}

/// A running librqbit session behind the [`TorrentEngine`] port.
pub struct LibrqbitEngine {
    session: Arc<Session>,
    active: Arc<Mutex<HashMap<InfoHash, Active>>>,
    /// Never written: magnet resolution uses storage that refuses all I/O.
    resolve_folder: PathBuf,
    allow_uploads: bool,
    /// Every librqbit stream holds one of the session's 8 blocking permits,
    /// which piece checks and writes also need; readers are capped below that.
    open_files: Arc<tokio::sync::Semaphore>,
}

const MAX_OPEN_FILES: usize = 4;
/// How long a new reader waits for a torrent to finish checking local bytes.
const INITIALIZE_WAIT: Duration = Duration::from_secs(120);

/// How long a read waits before asking librqbit to reconnect needed peers.
const RECONNECT_NUDGE: Duration = Duration::from_secs(3);

/// A librqbit stream plus its slot in the engine's reader limit.
///
/// A finished torrent (every selected file verified) disconnects its peers.
/// Opening a stream asks it to reconnect peers it no longer needed, but in
/// librqbit 9.0.1 a peer whose disconnect is still in progress at that moment
/// is missed and never retried, so the read would wait forever. While a read
/// waits, this reader therefore opens and drops a short-lived stream every
/// [`RECONNECT_NUDGE`], which repeats that request.
struct OpenFile<S> {
    stream: S,
    _permit: tokio::sync::OwnedSemaphorePermit,
    handle: Arc<ManagedTorrent>,
    file_index: usize,
    nudge: Option<std::pin::Pin<Box<tokio::time::Sleep>>>,
}

impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for OpenFile<S> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = &mut *self;
        let polled = std::pin::Pin::new(&mut this.stream).poll_read(cx, buf);
        if polled.is_ready() {
            this.nudge = None;
            return polled;
        }
        let nudge = this
            .nudge
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(RECONNECT_NUDGE)));
        if nudge.as_mut().poll(cx).is_ready() {
            this.nudge = None;
            let handle = Arc::clone(&this.handle);
            let file_index = this.file_index;
            tokio::spawn(async move {
                let _ = handle.stream(file_index).await;
            });
            // Arm the next nudge.
            cx.waker().wake_by_ref();
        }
        std::task::Poll::Pending
    }
}

impl<S: tokio::io::AsyncSeek + Unpin> tokio::io::AsyncSeek for OpenFile<S> {
    fn start_seek(
        mut self: std::pin::Pin<&mut Self>,
        position: std::io::SeekFrom,
    ) -> std::io::Result<()> {
        std::pin::Pin::new(&mut self.stream).start_seek(position)
    }

    fn poll_complete(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<u64>> {
        std::pin::Pin::new(&mut self.stream).poll_complete(cx)
    }
}

impl LibrqbitEngine {
    /// Start a session with explicit network options for `config.profile`.
    ///
    /// # Errors
    /// Returns [`EngineError::Engine`] when the session cannot start.
    pub async fn start(config: EngineConfig) -> Result<Self, EngineError> {
        std::fs::create_dir_all(&config.state_directory)
            .map_err(|error| EngineError::Engine(format!("cannot create engine state: {error}")))?;
        let options = session_options(&config);
        let resolve_folder = config.state_directory.join("resolving");
        let session = Session::new_with_opts(config.state_directory, options)
            .await
            .map_err(|error| engine_error(&error))?;
        Ok(Self {
            session,
            active: Arc::new(Mutex::new(HashMap::new())),
            resolve_folder,
            allow_uploads: config.allow_uploads,
            // At most MAX_OPEN_FILES readers plus one transient nudge stream
            // hold librqbit permits, leaving the rest for piece I/O.
            open_files: Arc::new(tokio::sync::Semaphore::new(MAX_OPEN_FILES)),
        })
    }

    /// Stop every transfer and close all sockets. Content is never deleted.
    pub async fn shutdown(&self) {
        self.lock().clear();
        self.session.stop().await;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<InfoHash, Active>> {
        self.active.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Read a `.torrent` without touching the network.
    async fn resolve_offline(&self, bytes: Vec<u8>) -> Result<ResolvedTorrent, EngineError> {
        let response = self
            .session
            .add_torrent(
                AddTorrent::from_bytes(bytes),
                Some(AddTorrentOptions {
                    list_only: true,
                    ..AddTorrentOptions::default()
                }),
            )
            .await
            .map_err(|error| engine_error(&error))?;
        match response {
            AddTorrentResponse::ListOnly(list) => resolved(
                &list.info_hash.as_string(),
                &list.info,
                list.torrent_bytes.to_vec(),
            ),
            AddTorrentResponse::Added(_, handle)
            | AddTorrentResponse::AlreadyManaged(_, handle) => resolved_from_handle(&handle),
        }
    }

    /// Fetch metadata from peers. librqbit's list-only mode never announces,
    /// so local service discovery would only hear the seeder's periodic
    /// announcement. The torrent is therefore added live with no file selected
    /// and storage that refuses all I/O, then removed once metadata is known.
    async fn resolve_from_peers(
        &self,
        uri: String,
        initial_peers: Vec<SocketAddr>,
    ) -> Result<ResolvedTorrent, EngineError> {
        let response = self
            .session
            .add_torrent(
                AddTorrent::from_url(uri),
                Some(AddTorrentOptions {
                    only_files: Some(Vec::new()),
                    storage_factory: Some(MetadataOnly.boxed()),
                    output_folder: Some(self.resolve_folder.to_string_lossy().into_owned()),
                    initial_peers: peers(initial_peers),
                    ..AddTorrentOptions::default()
                }),
            )
            .await
            .map_err(|error| engine_error(&error))?;
        let handle = match response {
            AddTorrentResponse::Added(_, handle) => handle,
            AddTorrentResponse::AlreadyManaged(_, handle) => return resolved_from_handle(&handle),
            AddTorrentResponse::ListOnly(_) => {
                return Err(EngineError::Engine(
                    "unexpected list-only response".to_owned(),
                ));
            }
        };
        // Removal runs to completion even if the caller stops waiting.
        let session = Arc::clone(&self.session);
        tokio::spawn(async move {
            // Metadata peers that reconnect once the torrent is live are
            // returned so a following fetch can dial them directly. If none
            // reconnect, the full wait also moves the fetch's discovery
            // announcement past the seeder's one-second reply rate limit.
            let deadline = tokio::time::Instant::now() + PEER_HINT_WAIT;
            while connected_peers(&handle).is_empty() && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(CHECK_POLL_INTERVAL).await;
            }
            let result = resolved_from_handle(&handle);
            session
                .delete(TorrentIdOrHash::Id(handle.id()), false)
                .await
                .map_err(|error| engine_error(&error))?;
            result
        })
        .await
        .map_err(|error| EngineError::Engine(error.to_string()))?
    }
}

fn resolved_from_handle(handle: &ManagedTorrent) -> Result<ResolvedTorrent, EngineError> {
    let mut result = handle
        .with_metadata(|metadata| {
            resolved(
                &handle.info_hash().as_string(),
                &metadata.info,
                metadata.torrent_bytes.to_vec(),
            )
        })
        .map_err(|error| engine_error(&error))??;
    result.peers = connected_peers(handle);
    Ok(result)
}

#[expect(
    clippy::default_trait_access,
    reason = "librqbit exports PeerStatsFilter only with its HTTP API feature"
)]
fn connected_peers(handle: &ManagedTorrent) -> Vec<SocketAddr> {
    handle.live().map_or_else(Vec::new, |live| {
        let mut peers: Vec<SocketAddr> = live
            .per_peer_stats_snapshot(Default::default())
            .peers
            .keys()
            .filter_map(|address| address.parse().ok())
            .collect();
        peers.sort_unstable();
        peers
    })
}

fn resolved(
    info_hash: &str,
    info: &librqbit::ValidatedTorrentMetaV1Info<librqbit::ByteBufOwned>,
    metainfo: Vec<u8>,
) -> Result<ResolvedTorrent, EngineError> {
    let files = info
        .iter_file_details()
        .enumerate()
        .map(|(index, details)| TorrentFile {
            index,
            path: details.filename.to_vec().join("/"),
            length: details.len,
        })
        .collect();
    Ok(ResolvedTorrent {
        info_hash: InfoHash::parse(info_hash)?,
        name: info
            .name()
            .map(std::borrow::Cow::into_owned)
            .unwrap_or_default(),
        files,
        metainfo,
        peers: Vec::new(),
    })
}

impl TorrentEngine for LibrqbitEngine {
    async fn seed(&self, request: SeedRequest) -> Result<InfoHash, EngineError> {
        if !self.allow_uploads {
            return Err(EngineError::UnsupportedCapability(
                "seeding while uploads are disabled",
            ));
        }
        let parsed = librqbit::torrent_from_bytes(&request.torrent)
            .map_err(|error| EngineError::InvalidTorrent(error.to_string()))?;
        let info_hash = InfoHash::parse(&parsed.info_hash.as_string())?;
        if self.lock().contains_key(&info_hash) {
            return Err(EngineError::AlreadyActive);
        }
        let output_folder = request.content_root.to_string_lossy().into_owned();
        let response = self
            .session
            .add_torrent(
                AddTorrent::from_bytes(request.torrent),
                Some(AddTorrentOptions {
                    output_folder: Some(output_folder),
                    storage_factory: Some(
                        ReadOnlyContent {
                            root: request.content_root,
                        }
                        .boxed(),
                    ),
                    overwrite: false,
                    ..AddTorrentOptions::default()
                }),
            )
            .await
            .map_err(|error| {
                match error
                    .chain()
                    .find_map(|cause| cause.downcast_ref::<ContentMismatch>())
                {
                    Some(mismatch) => EngineError::ContentMismatch(mismatch.0.clone()),
                    None => engine_error(&error),
                }
            })?;
        let handle = match response {
            AddTorrentResponse::Added(_, handle) => handle,
            AddTorrentResponse::AlreadyManaged(..) => return Err(EngineError::AlreadyActive),
            AddTorrentResponse::ListOnly(_) => {
                return Err(EngineError::Engine(
                    "unexpected list-only response".to_owned(),
                ));
            }
        };
        let failure = Arc::new(Mutex::new(None));
        self.lock().insert(
            info_hash.clone(),
            Active {
                handle: Arc::clone(&handle),
                failure: Arc::clone(&failure),
            },
        );
        tokio::spawn(confirm_complete(Arc::clone(&self.session), handle, failure));
        Ok(info_hash)
    }

    async fn resolve(&self, request: ResolveRequest) -> Result<ResolvedTorrent, EngineError> {
        match request.source {
            TorrentSource::Metainfo(bytes) => {
                librqbit::torrent_from_bytes(&bytes)
                    .map_err(|error| EngineError::InvalidTorrent(error.to_string()))?;
                self.resolve_offline(bytes).await
            }
            TorrentSource::Magnet(uri) => {
                let info_hash = librqbit::Magnet::parse(&uri)
                    .ok()
                    .and_then(|magnet| magnet.as_id20())
                    .ok_or_else(|| {
                        EngineError::InvalidTorrent(
                            "the magnet link has no v1 info hash".to_owned(),
                        )
                    })?;
                if let Some(handle) = self.session.get(TorrentIdOrHash::Hash(info_hash)) {
                    return resolved_from_handle(&handle);
                }
                self.resolve_from_peers(uri, request.initial_peers).await
            }
        }
    }

    async fn fetch(&self, request: FetchRequest) -> Result<InfoHash, EngineError> {
        let parsed = librqbit::torrent_from_bytes(&request.metainfo)
            .map_err(|error| EngineError::InvalidTorrent(error.to_string()))?;
        let info_hash = InfoHash::parse(&parsed.info_hash.as_string())?;
        let active = self
            .lock()
            .get(&info_hash)
            .map(|active| Arc::clone(&active.handle));
        if let Some(handle) = active {
            let selection: HashSet<usize> = request.files.into_iter().collect();
            self.session
                .update_only_files(&handle, &selection)
                .await
                .map_err(|error| engine_error(&error))?;
            return Ok(info_hash);
        }
        std::fs::create_dir_all(&request.content_root).map_err(|error| {
            EngineError::Engine(format!("cannot create the download directory: {error}"))
        })?;
        let response = self
            .session
            .add_torrent(
                AddTorrent::from_bytes(request.metainfo),
                Some(AddTorrentOptions {
                    only_files: Some(request.files),
                    output_folder: Some(request.content_root.to_string_lossy().into_owned()),
                    overwrite: true,
                    initial_peers: peers(request.initial_peers),
                    ..AddTorrentOptions::default()
                }),
            )
            .await
            .map_err(|error| engine_error(&error))?;
        let (AddTorrentResponse::Added(_, handle) | AddTorrentResponse::AlreadyManaged(_, handle)) =
            response
        else {
            return Err(EngineError::Engine(
                "unexpected list-only response".to_owned(),
            ));
        };
        self.lock().insert(
            info_hash.clone(),
            Active {
                handle,
                failure: Arc::new(Mutex::new(None)),
            },
        );
        Ok(info_hash)
    }

    /// Uses `Session::update_only_files` (librqbit 9.0.1 src/session.rs),
    /// which re-queues newly selected pieces that are not verified yet.
    /// Session persistence is off (ADR 0004 decision 3), so callers re-apply
    /// selections after a restart.
    async fn select_files(&self, info_hash: &InfoHash, files: &[usize]) -> Result<(), EngineError> {
        let handle = self
            .lock()
            .get(info_hash)
            .map(|active| Arc::clone(&active.handle))
            .ok_or(EngineError::NotActive)?;
        let file_count = handle
            .with_metadata(|metadata| metadata.file_infos.len())
            .map_err(|error| engine_error(&error))?;
        if let Some(index) = files.iter().find(|&&index| index >= file_count) {
            return Err(EngineError::InvalidTorrent(format!(
                "the torrent has no file {index}"
            )));
        }
        if files.is_empty() {
            return Err(EngineError::InvalidTorrent(
                "select at least one file".to_owned(),
            ));
        }
        tokio::time::timeout(INITIALIZE_WAIT, handle.wait_until_initialized())
            .await
            .map_err(|_| {
                EngineError::Engine("the torrent is still checking its local files".to_owned())
            })?
            .map_err(|error| engine_error(&error))?;
        self.session
            .update_only_files(&handle, &files.iter().copied().collect())
            .await
            .map_err(|error| engine_error(&error))
    }

    async fn stop(&self, info_hash: &InfoHash) -> Result<(), EngineError> {
        let active = self
            .lock()
            .remove(info_hash)
            .ok_or(EngineError::NotActive)?;
        self.session
            .delete(TorrentIdOrHash::Id(active.handle.id()), false)
            .await
            .map_err(|error| engine_error(&error))
    }

    async fn open_file(
        &self,
        info_hash: &InfoHash,
        file_index: usize,
    ) -> Result<Box<dyn VerifiedFile>, EngineError> {
        let handle = self
            .lock()
            .get(info_hash)
            .map(|active| Arc::clone(&active.handle))
            .ok_or(EngineError::NotActive)?;
        let file_count = handle
            .with_metadata(|metadata| metadata.file_infos.len())
            .map_err(|error| engine_error(&error))?;
        if file_index >= file_count {
            return Err(EngineError::InvalidTorrent(format!(
                "the torrent has no file {file_index}"
            )));
        }
        // A torrent added moments ago is still checking local bytes, and
        // librqbit refuses to stream until it leaves that state
        // (`ManagedTorrent::wait_until_initialized`, librqbit 9.0.1
        // src/torrent_state/mod.rs). Wait for it, bounded, before taking a
        // reader permit.
        tokio::time::timeout(INITIALIZE_WAIT, handle.wait_until_initialized())
            .await
            .map_err(|_| {
                EngineError::Engine("the torrent is still checking its local files".to_owned())
            })?
            .map_err(|error| engine_error(&error))?;
        let permit = Arc::clone(&self.open_files)
            .try_acquire_owned()
            .map_err(|_| {
                EngineError::Engine("too many torrent files are open for reading".to_owned())
            })?;
        // FileStream reads only verified pieces and prioritizes a 32 MiB
        // window ahead of its position, also for unselected files.
        let stream = Arc::clone(&handle)
            .stream(file_index)
            .await
            .map_err(|error| engine_error(&error))?;
        Ok(Box::new(OpenFile {
            stream,
            _permit: permit,
            handle,
            file_index,
            nudge: None,
        }))
    }

    fn transfer(&self, info_hash: &InfoHash) -> Option<TransferStats> {
        self.lock()
            .get(info_hash)
            .map(|active| transfer_stats(info_hash, active))
    }

    fn transfers(&self) -> Vec<TransferStats> {
        let mut transfers: Vec<_> = self
            .lock()
            .iter()
            .map(|(info_hash, active)| transfer_stats(info_hash, active))
            .collect();
        transfers.sort_by(|left, right| left.info_hash.cmp(&right.info_hash));
        transfers
    }

    fn listen_address(&self) -> Option<SocketAddr> {
        self.session.listen_addr()
    }
}

/// Wait for librqbit's initial piece check. Content that does not verify
/// completely is paused so that nothing is served and no write is attempted.
async fn confirm_complete(
    session: Arc<Session>,
    handle: Arc<ManagedTorrent>,
    failure: Arc<Mutex<Option<String>>>,
) {
    loop {
        let stats = handle.stats();
        match stats.state {
            TorrentStatsState::Initializing { .. } => {
                tokio::time::sleep(CHECK_POLL_INTERVAL).await;
            }
            TorrentStatsState::Error => {
                *failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(
                    stats
                        .error
                        .unwrap_or_else(|| "the torrent engine reported an error".to_owned()),
                );
                return;
            }
            TorrentStatsState::Live | TorrentStatsState::Paused => {
                if !stats.finished {
                    *failure.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some(CONTENT_CHANGED.to_owned());
                    let _ = session.pause(&handle).await;
                }
                return;
            }
        }
    }
}

fn transfer_stats(info_hash: &InfoHash, active: &Active) -> TransferStats {
    let engine_stats = active.handle.stats();
    let failure = active
        .failure
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let state = match (&failure, &engine_stats.state) {
        (Some(_), _) | (None, TorrentStatsState::Error) => TransferState::Failed,
        (None, TorrentStatsState::Initializing { .. }) => TransferState::Checking,
        (None, TorrentStatsState::Live) if engine_stats.finished => TransferState::Seeding,
        (None, _) => TransferState::Downloading,
    };
    let live = engine_stats.live.as_ref();
    TransferStats {
        info_hash: info_hash.clone(),
        state,
        verified_bytes: engine_stats.progress_bytes,
        total_bytes: engine_stats.total_bytes,
        uploaded_bytes: engine_stats.uploaded_bytes,
        upload_bytes_per_second: live.map_or(0, |live| live.upload_speed.as_bytes()),
        downloaded_bytes: live.map_or(0, |live| live.snapshot.downloaded_and_checked_bytes),
        connected_peers: live.map_or(0, |live| live.snapshot.peer_stats.live),
        file_verified_bytes: engine_stats.file_progress.clone(),
        failure: failure.or(engine_stats.error),
    }
}

fn session_options(config: &EngineConfig) -> SessionOptions {
    let reachability = config.reachability.for_profile(config.profile);
    let port = reachability.port.unwrap_or(0);
    let loopback = config.profile == NetworkProfile::Loopback;
    let internet = config.profile == NetworkProfile::Internet;
    let listen_addr = if loopback {
        SocketAddr::from((Ipv4Addr::LOCALHOST, port))
    } else {
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, port))
    };
    SessionOptions {
        dht: internet.then_some(DhtSessionConfig {
            bootstrap_addrs: None,
            port: None,
            persistence: None,
        }),
        bind_device_name: loopback_device(loopback),
        // Trackers are usually public servers, so only the Internet profile
        // may announce to them, whatever a torrent or magnet lists.
        disable_trackers: !internet,
        disable_local_service_discovery: loopback,
        fastresume: false,
        persistence: None,
        ipv4_only: loopback,
        listen: Some(ListenerOptions {
            mode: ListenerMode::TcpOnly,
            listen_addr,
            enable_upnp_port_forwarding: reachability.port_forwarding,
            ipv4_only: loopback,
            ..ListenerOptions::default()
        }),
        disable_upload: !config.allow_uploads,
        ..SessionOptions::default()
    }
}

/// On Linux, loopback sessions bind every socket (including librqbit's
/// always-created UDP tracker socket) to `lo` with `SO_BINDTODEVICE`.
fn loopback_device(loopback: bool) -> Option<String> {
    (loopback && cfg!(target_os = "linux")).then(|| "lo".to_owned())
}

fn peers(initial_peers: Vec<SocketAddr>) -> Option<Vec<SocketAddr>> {
    (!initial_peers.is_empty()).then_some(initial_peers)
}

fn engine_error(error: &anyhow::Error) -> EngineError {
    EngineError::Engine(format!("{error:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(profile: NetworkProfile, reachability: PeerReachability) -> ListenerOptions {
        session_options(&EngineConfig {
            profile,
            state_directory: PathBuf::new(),
            reachability,
            allow_uploads: true,
        })
        .listen
        .unwrap()
    }

    #[test]
    fn only_the_internet_profile_announces_to_trackers_and_the_dht() {
        for (profile, internet) in [
            (NetworkProfile::Loopback, false),
            (NetworkProfile::LocalNetwork, false),
            (NetworkProfile::Internet, true),
        ] {
            let options = session_options(&EngineConfig {
                profile,
                state_directory: PathBuf::new(),
                reachability: PeerReachability::default(),
                allow_uploads: true,
            });
            assert_eq!(options.disable_trackers, !internet, "{profile:?}");
            assert_eq!(options.dht.is_some(), internet, "{profile:?}");
        }
    }

    #[test]
    fn port_forwarding_and_fixed_ports_follow_the_profile() {
        let chosen = PeerReachability::new(Some(51413), true).unwrap();
        let internet = listener(NetworkProfile::Internet, chosen);
        assert_eq!(internet.listen_addr.port(), 51413);
        assert!(internet.enable_upnp_port_forwarding);

        let local = listener(NetworkProfile::LocalNetwork, chosen);
        assert_eq!(local.listen_addr.port(), 51413);
        assert!(!local.enable_upnp_port_forwarding);

        let loopback = listener(NetworkProfile::Loopback, chosen);
        assert_eq!(
            loopback.listen_addr,
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
        );
        assert!(!loopback.enable_upnp_port_forwarding);

        let ephemeral = listener(NetworkProfile::Internet, PeerReachability::default());
        assert_eq!(ephemeral.listen_addr.port(), 0);
        assert!(!ephemeral.enable_upnp_port_forwarding);
    }
}
