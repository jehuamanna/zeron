//! The running seed server: one engine that uploads, a task per listed
//! torrent, and a read-only health endpoint.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime},
};

use serde::Serialize;
use tokio::task::JoinHandle;
use torrent_librqbit::{EngineConfig, LibrqbitEngine};
use torrent_port::{
    EngineError, FetchRequest, InfoHash, NetworkProfile, PeerReachability, RateLimits,
    ResolveRequest, SeedRequest, TorrentEngine, TorrentSource, TransferState,
};

use crate::registry::{Entry, Registry, Source};

/// How the server runs.
#[derive(Debug, Clone)]
pub struct ServeConfig {
    /// Which networks the engine uses. `Internet` for a public hub.
    pub profile: NetworkProfile,
    /// Fixed peer port; `None` picks a new one on each start.
    pub port: Option<u16>,
    /// Ask the router to open the peer port (PCP, NAT-PMP, `UPnP`).
    /// Only for servers behind a home router; a public server needs none.
    pub map_ports: bool,
    /// Session-wide speed caps.
    pub limits: RateLimits,
    /// How often the seed list is checked for changes.
    pub poll: Duration,
    /// How long one attempt to fetch a magnet's metadata may take.
    pub resolve_timeout: Duration,
    /// Wait before trying a failed torrent again.
    pub retry_after: Duration,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            profile: NetworkProfile::Internet,
            port: Some(51413),
            map_ports: false,
            limits: RateLimits::default(),
            poll: Duration::from_secs(5),
            resolve_timeout: Duration::from_mins(10),
            retry_after: Duration::from_mins(1),
        }
    }
}

/// Why the server could not start.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// The engine did not start, for example because the port is taken.
    #[error("the torrent engine did not start: {0}")]
    Engine(#[from] EngineError),
    /// The data directory is unusable.
    #[error(transparent)]
    Registry(#[from] crate::registry::RegistryError),
}

/// What a torrent's task is doing, beside the engine's own state.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// Waiting for a peer to send the torrent's metadata.
    FindingMetadata,
    /// In the engine: checking, downloading, or seeding.
    Active,
    /// The last attempt failed; it is retried.
    Retrying(String),
}

/// The seed server.
pub struct Server {
    engine: Arc<LibrqbitEngine>,
    registry: Registry,
    config: ServeConfig,
    phases: Arc<Mutex<HashMap<String, Phase>>>,
    tasks: Mutex<HashMap<String, (Entry, JoinHandle<()>)>>,
    seen: Mutex<Option<SystemTime>>,
    mapper: Option<port_mapping::PortMapper>,
}

impl Server {
    /// Start the engine and resume every listed torrent.
    ///
    /// # Errors
    ///
    /// See [`ServeError`].
    pub async fn start(registry: Registry, config: ServeConfig) -> Result<Arc<Self>, ServeError> {
        let engine = Arc::new(
            LibrqbitEngine::start(EngineConfig {
                profile: config.profile,
                state_directory: registry.engine_directory(),
                reachability: PeerReachability {
                    port: config.port,
                    port_forwarding: false,
                },
                allow_uploads: true,
            })
            .await?,
        );
        engine.set_rate_limits(config.limits)?;
        let mapper = (config.map_ports && config.profile == NetworkProfile::Internet)
            .then(|| engine.listen_address())
            .flatten()
            .map(|address| {
                let (mapper, run) = port_mapping::PortMapper::new(
                    port_mapping::MapperConfig {
                        description: "Zeron seed server".to_owned(),
                        ..port_mapping::MapperConfig::default()
                    },
                    vec![address.port()],
                );
                tokio::spawn(run);
                mapper
            });
        let server = Arc::new(Self {
            engine,
            registry,
            config,
            phases: Arc::new(Mutex::new(HashMap::new())),
            tasks: Mutex::new(HashMap::new()),
            seen: Mutex::new(None),
            mapper,
        });
        server.reconcile().await?;
        Ok(server)
    }

    /// The peer address the engine listens on.
    #[must_use]
    pub fn listen_address(&self) -> Option<SocketAddr> {
        self.engine.listen_address()
    }

    /// Start torrents added to the list and stop those removed since the
    /// last call. Unchanged lists cost one file-metadata read.
    ///
    /// # Errors
    ///
    /// An unreadable list; running torrents are left as they are.
    pub async fn reconcile(self: &Arc<Self>) -> Result<(), ServeError> {
        let modified = self.registry.modified();
        {
            let mut seen = lock(&self.seen);
            if seen.is_some() && *seen == modified {
                return Ok(());
            }
            *seen = modified;
        }
        let entries = self.registry.entries()?;
        let removed: Vec<String> = {
            let tasks = lock(&self.tasks);
            tasks
                .keys()
                .filter(|hash| !entries.iter().any(|entry| &entry.info_hash == *hash))
                .cloned()
                .collect()
        };
        for hash in removed {
            if let Some((_, task)) = lock(&self.tasks).remove(&hash) {
                task.abort();
            }
            lock(&self.phases).remove(&hash);
            if let Ok(info_hash) = InfoHash::parse(&hash) {
                let _ = self.engine.stop(&info_hash).await;
            }
            eprintln!("stopped {hash}");
        }
        for entry in entries {
            let mut tasks = lock(&self.tasks);
            if tasks.contains_key(&entry.info_hash) {
                continue;
            }
            let task = tokio::spawn(Arc::clone(self).keep_seeding(entry.clone()));
            tasks.insert(entry.info_hash.clone(), (entry, task));
        }
        Ok(())
    }

    /// Get one torrent into the engine, retrying until it is there.
    async fn keep_seeding(self: Arc<Self>, entry: Entry) {
        let hash = entry.info_hash.clone();
        loop {
            match self.add(&entry).await {
                Ok(()) => {
                    self.set_phase(&hash, Phase::Active);
                    eprintln!("serving {hash}");
                    return;
                }
                Err(reason) => {
                    eprintln!("{hash}: {reason}; retrying");
                    self.set_phase(&hash, Phase::Retrying(reason));
                    tokio::time::sleep(self.config.retry_after).await;
                }
            }
        }
    }

    async fn add(&self, entry: &Entry) -> Result<(), String> {
        let hash = &entry.info_hash;
        match &entry.source {
            Source::Release { content_root } => {
                let torrent = self
                    .registry
                    .metainfo(hash)
                    .ok_or("the release's .torrent is missing")?;
                accept(
                    self.engine
                        .seed(SeedRequest {
                            torrent,
                            content_root: content_root.clone(),
                        })
                        .await,
                )
            }
            Source::Follow { magnet, peers } => {
                let metainfo = self.follow_metainfo(hash, magnet, peers).await?;
                // Read the file list offline from the stored metadata.
                let files = self
                    .engine
                    .resolve(ResolveRequest {
                        source: TorrentSource::Metainfo(metainfo.clone()),
                        initial_peers: Vec::new(),
                    })
                    .await
                    .map_err(|error| error.to_string())?
                    .files
                    .iter()
                    .map(|file| file.index)
                    .collect();
                accept(
                    self.engine
                        .fetch(FetchRequest {
                            metainfo,
                            content_root: self.registry.content_directory(hash),
                            files,
                            initial_peers: peers.clone(),
                        })
                        .await,
                )
            }
        }
    }

    /// Stored metadata, or metadata fetched from the swarm and then stored.
    async fn follow_metainfo(
        &self,
        hash: &str,
        magnet: &str,
        peers: &[SocketAddr],
    ) -> Result<Vec<u8>, String> {
        if let Some(metainfo) = self.registry.metainfo(hash) {
            return Ok(metainfo);
        }
        self.set_phase(hash, Phase::FindingMetadata);
        let resolved = tokio::time::timeout(
            self.config.resolve_timeout,
            self.engine.resolve(ResolveRequest {
                source: TorrentSource::Magnet(magnet.to_owned()),
                initial_peers: peers.to_vec(),
            }),
        )
        .await
        .map_err(|_| "no peer sent the torrent's metadata yet".to_owned())?
        .map_err(|error| error.to_string())?;
        if resolved.info_hash.as_str() != hash {
            return Err("the metadata is for another torrent".to_owned());
        }
        self.registry
            .save_metainfo(hash, &resolved.metainfo)
            .map_err(|error| error.to_string())?;
        Ok(resolved.metainfo)
    }

    fn set_phase(&self, hash: &str, phase: Phase) {
        lock(&self.phases).insert(hash.to_owned(), phase);
    }

    /// What every listed torrent is doing, for the health endpoint.
    #[must_use]
    pub fn health(&self) -> Health {
        let phases = lock(&self.phases).clone();
        let mut torrents: Vec<TorrentHealth> = lock(&self.tasks)
            .values()
            .map(|(entry, _)| {
                let stats = InfoHash::parse(&entry.info_hash)
                    .ok()
                    .and_then(|hash| self.engine.transfer(&hash));
                let phase = phases.get(&entry.info_hash);
                TorrentHealth {
                    info_hash: entry.info_hash.clone(),
                    mode: match entry.source {
                        Source::Follow { .. } => "follow",
                        Source::Release { .. } => "release",
                    },
                    state: match (phase, stats.as_ref().map(|stats| stats.state)) {
                        (Some(Phase::FindingMetadata), _) => "finding_metadata",
                        (Some(Phase::Retrying(_)), _) => "retrying",
                        (_, Some(TransferState::Checking)) => "checking",
                        (_, Some(TransferState::Downloading)) => "downloading",
                        (_, Some(TransferState::Seeding)) => "seeding",
                        (_, Some(TransferState::Paused)) => "paused",
                        (_, Some(TransferState::Failed)) => "failed",
                        _ => "starting",
                    },
                    problem: match phase {
                        Some(Phase::Retrying(reason)) => Some(reason.clone()),
                        _ => stats.as_ref().and_then(|stats| stats.failure.clone()),
                    },
                    verified_bytes: stats.as_ref().map_or(0, |stats| stats.verified_bytes),
                    total_bytes: stats.as_ref().map_or(0, |stats| stats.total_bytes),
                    uploaded_bytes: stats.as_ref().map_or(0, |stats| stats.uploaded_bytes),
                    upload_bytes_per_second: stats
                        .as_ref()
                        .map_or(0, |stats| stats.upload_bytes_per_second),
                    connected_peers: stats.as_ref().map_or(0, |stats| stats.connected_peers),
                }
            })
            .collect();
        torrents.sort_by(|a, b| a.info_hash.cmp(&b.info_hash));
        Health {
            status: "ok",
            listen_port: self.listen_address().map(|address| address.port()),
            router: self.mapper.as_ref().map(|mapper| {
                let status = mapper.status();
                match (status.state, status.protocol) {
                    (port_mapping::MappingState::Mapped, Some(protocol)) => {
                        format!("open via {}", protocol.name())
                    }
                    (port_mapping::MappingState::Probing, _) => "asking".to_owned(),
                    _ => "not opened".to_owned(),
                }
            }),
            torrents,
        }
    }

    /// Remove router mappings and stop the engine. Listed torrents stay
    /// listed and resume on the next start.
    pub async fn shutdown(&self) {
        for (_, (_, task)) in lock(&self.tasks).drain() {
            task.abort();
        }
        if let Some(mapper) = &self.mapper {
            mapper.shutdown(Duration::from_secs(3)).await;
        }
        self.engine.shutdown().await;
    }
}

fn accept(result: Result<InfoHash, EngineError>) -> Result<(), String> {
    match result {
        Ok(_) | Err(EngineError::AlreadyActive) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The health endpoint's answer.
#[derive(Debug, Clone, Serialize)]
pub struct Health {
    /// Always `ok` while the server answers.
    pub status: &'static str,
    /// The peer port.
    pub listen_port: Option<u16>,
    /// What the router did with the port, when asked to open it.
    pub router: Option<String>,
    /// Every listed torrent.
    pub torrents: Vec<TorrentHealth>,
}

/// One torrent's health. Metrics are this process's own.
#[derive(Debug, Clone, Serialize)]
pub struct TorrentHealth {
    /// Its info hash.
    pub info_hash: String,
    /// `follow` or `release`.
    pub mode: &'static str,
    /// `finding_metadata`, `retrying`, `starting`, `checking`,
    /// `downloading`, `seeding`, `paused`, or `failed`.
    pub state: &'static str,
    /// Why it is retrying or failed.
    pub problem: Option<String>,
    /// Bytes verified on this server.
    pub verified_bytes: u64,
    /// Total bytes.
    pub total_bytes: u64,
    /// Bytes uploaded since the server started.
    pub uploaded_bytes: u64,
    /// Current upload rate.
    pub upload_bytes_per_second: u64,
    /// Connected peers.
    pub connected_peers: u32,
}

/// Serve `GET /health` on `address` until `shutdown` resolves. The endpoint
/// is read-only; bind it to loopback unless a monitor elsewhere needs it.
///
/// # Errors
///
/// The address cannot be bound.
pub async fn serve_health(
    server: Arc<Server>,
    address: SocketAddr,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<SocketAddr> {
    use axum::{Json, Router, routing::get};
    let listener = tokio::net::TcpListener::bind(address).await?;
    let bound = listener.local_addr()?;
    let app = Router::new().route(
        "/health",
        get(move || {
            let server = Arc::clone(&server);
            async move { Json(server.health()) }
        }),
    );
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await;
    });
    Ok(bound)
}
