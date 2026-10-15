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
use torrent_port::{
    EngineError, FetchRequest, InfoHash, NetworkProfile, ResolveRequest, ResolvedTorrent,
    SeedRequest, TorrentEngine, TorrentFile, TorrentSource, TransferState, TransferStats,
};

use crate::storage::{ContentMismatch, ReadOnlyContent};

const CHECK_POLL_INTERVAL: Duration = Duration::from_millis(100);
const CONTENT_CHANGED: &str =
    "the content on disk no longer matches the torrent; nothing is being served";

/// Engine construction parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    /// Which networks the engine may use.
    pub profile: NetworkProfile,
    /// Private directory for engine state. Seeding never writes content here.
    pub state_directory: PathBuf,
    /// Fixed peer port, or `None` for an ephemeral port.
    pub listen_port: Option<u16>,
}

struct Active {
    handle: Arc<ManagedTorrent>,
    failure: Arc<Mutex<Option<String>>>,
}

/// A running librqbit session behind the [`TorrentEngine`] port.
pub struct LibrqbitEngine {
    session: Arc<Session>,
    active: Arc<Mutex<HashMap<InfoHash, Active>>>,
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
        let session = Session::new_with_opts(config.state_directory, options)
            .await
            .map_err(|error| engine_error(&error))?;
        Ok(Self {
            session,
            active: Arc::new(Mutex::new(HashMap::new())),
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
}

impl TorrentEngine for LibrqbitEngine {
    async fn seed(&self, request: SeedRequest) -> Result<InfoHash, EngineError> {
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
        let add = match request.source {
            TorrentSource::Metainfo(bytes) => {
                librqbit::torrent_from_bytes(&bytes)
                    .map_err(|error| EngineError::InvalidTorrent(error.to_string()))?;
                AddTorrent::from_bytes(bytes)
            }
            TorrentSource::Magnet(uri) => {
                librqbit::Magnet::parse(&uri)
                    .ok()
                    .and_then(|magnet| magnet.as_id20())
                    .ok_or_else(|| {
                        EngineError::InvalidTorrent(
                            "the magnet link has no v1 info hash".to_owned(),
                        )
                    })?;
                AddTorrent::from_url(uri)
            }
        };
        let response = self
            .session
            .add_torrent(
                add,
                Some(AddTorrentOptions {
                    list_only: true,
                    initial_peers: peers(request.initial_peers),
                    ..AddTorrentOptions::default()
                }),
            )
            .await
            .map_err(|error| engine_error(&error))?;
        let AddTorrentResponse::ListOnly(list) = response else {
            return Err(EngineError::Engine(
                "the torrent is already active in this engine".to_owned(),
            ));
        };
        let files = list
            .info
            .iter_file_details()
            .enumerate()
            .map(|(index, details)| TorrentFile {
                index,
                path: details.filename.to_vec().join("/"),
                length: details.len,
            })
            .collect();
        Ok(ResolvedTorrent {
            info_hash: InfoHash::parse(&list.info_hash.as_string())?,
            name: list
                .info
                .name()
                .map(std::borrow::Cow::into_owned)
                .unwrap_or_default(),
            files,
            metainfo: list.torrent_bytes.to_vec(),
        })
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
    let port = config.listen_port.unwrap_or(0);
    let loopback = config.profile == NetworkProfile::Loopback;
    let listen_addr = if loopback {
        SocketAddr::from((Ipv4Addr::LOCALHOST, port))
    } else {
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, port))
    };
    SessionOptions {
        dht: (config.profile == NetworkProfile::Internet).then_some(DhtSessionConfig {
            bootstrap_addrs: None,
            port: None,
            persistence: None,
        }),
        bind_device_name: loopback_device(loopback),
        disable_trackers: loopback,
        disable_local_service_discovery: loopback,
        fastresume: false,
        persistence: None,
        ipv4_only: loopback,
        listen: Some(ListenerOptions {
            mode: ListenerMode::TcpOnly,
            listen_addr,
            enable_upnp_port_forwarding: false,
            ipv4_only: loopback,
            ..ListenerOptions::default()
        }),
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
