//! Asks the router to open Zeron's peer ports, for TCP and UDP, with PCP
//! (RFC 6887), NAT-PMP (RFC 6886), or `UPnP` IGD, in that order, as common
//! torrent clients do. Mappings are renewed at half their lifetime and
//! removed at shutdown. The status reports what each protocol answered.
//!
//! This crate knows nothing about torrents: it maps the ports it is given.

mod upnp;
pub mod wire;

use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use thiserror::Error;
use tokio::{net::UdpSocket, sync::watch};
use tokio_util::sync::CancellationToken;

pub use upnp::SSDP_MULTICAST;
use wire::Reply;

/// A transport a port is opened for. Torrent peers use TCP and, for uTP, UDP
/// on the same port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transport {
    /// TCP.
    Tcp,
    /// UDP.
    Udp,
}

/// Both transports, in the order they are mapped.
pub const TRANSPORTS: [Transport; 2] = [Transport::Tcp, Transport::Udp];

/// A port-mapping protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    /// Port Control Protocol, RFC 6887.
    Pcp,
    /// NAT Port Mapping Protocol, RFC 6886.
    NatPmp,
    /// `UPnP` Internet Gateway Device.
    Upnp,
}

impl Protocol {
    /// Stable lowercase name for storage and IPC.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pcp => "pcp",
            Self::NatPmp => "nat_pmp",
            Self::Upnp => "upnp",
        }
    }
}

/// The order protocols are tried in.
pub const PROTOCOLS: [Protocol; 3] = [Protocol::Pcp, Protocol::NatPmp, Protocol::Upnp];

/// Why one protocol could not map a port.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MapError {
    /// Nothing answered in time.
    #[error("no answer")]
    NoAnswer,
    /// The router refused with this protocol's result or error code.
    #[error("refused with code {0}")]
    Refused(u32),
    /// The router speaks another version of the protocol.
    #[error("unsupported by the router")]
    Unsupported,
    /// The router's answer could not be understood.
    #[error("malformed answer")]
    Malformed,
    /// No default gateway was found.
    #[error("no default gateway")]
    NoGateway,
    /// A local socket failed.
    #[error("network error: {0}")]
    Io(String),
}

impl From<std::io::Error> for MapError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.kind().to_string())
    }
}

/// How the mapper is set up. Defaults suit a real network; tests point it
/// at fake servers on loopback.
#[derive(Debug, Clone)]
pub struct MapperConfig {
    /// The router; `None` asks the operating system for the default gateway.
    pub gateway: Option<Ipv4Addr>,
    /// Where NAT-PMP and PCP requests go on the router.
    pub pmp_port: u16,
    /// Where SSDP discovery goes.
    pub ssdp_target: SocketAddr,
    /// Lifetime requested for each mapping, in seconds.
    pub lifetime: u32,
    /// First wait for a NAT-PMP or PCP reply; it doubles on each of three tries.
    pub reply_timeout: Duration,
    /// How long `UPnP` discovery waits for gateways.
    pub discovery_wait: Duration,
    /// How long to wait before asking again after no protocol worked.
    pub retry_after: Duration,
    /// Shown in the router's `UPnP` mapping list.
    pub description: String,
}

impl Default for MapperConfig {
    fn default() -> Self {
        Self {
            gateway: None,
            pmp_port: wire::SERVER_PORT,
            ssdp_target: SSDP_MULTICAST,
            lifetime: 7200,
            reply_timeout: Duration::from_millis(250),
            discovery_wait: Duration::from_millis(2500),
            retry_after: Duration::from_secs(300),
            description: "Zeron".to_owned(),
        }
    }
}

/// What the mapper is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingState {
    /// Asking the router.
    Probing,
    /// Every port is open for TCP and UDP.
    Mapped,
    /// No protocol opened every port; it asks again later.
    Unavailable,
    /// Shut down; mappings were removed.
    Stopped,
}

/// One open port on the router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MappedPort {
    /// The port on this computer.
    pub internal_port: u16,
    /// The port peers reach on the router.
    pub external_port: u16,
    /// TCP or UDP.
    pub transport: Transport,
}

/// The mapper's current state, with what each protocol answered last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingStatus {
    /// What the mapper is doing.
    pub state: MappingState,
    /// The protocol holding the mappings, while mapped.
    pub protocol: Option<Protocol>,
    /// Open ports, while mapped.
    pub ports: Vec<MappedPort>,
    /// Each protocol tried in the last attempt and why it did not map, or
    /// `None` when it did.
    pub tried: Vec<(Protocol, Option<MapError>)>,
}

#[derive(Debug, Clone, Copy)]
struct Lease {
    transport: Transport,
    internal_port: u16,
    external_port: u16,
    /// Seconds granted; 0 is a permanent `UPnP` mapping.
    lifetime: u32,
    nonce: [u8; 12],
}

enum Session {
    Pcp {
        router: SocketAddr,
        client: Ipv4Addr,
    },
    NatPmp {
        router: SocketAddr,
    },
    Upnp {
        gateway: upnp::Gateway,
        client: Ipv4Addr,
    },
}

impl Session {
    const fn protocol(&self) -> Protocol {
        match self {
            Self::Pcp { .. } => Protocol::Pcp,
            Self::NatPmp { .. } => Protocol::NatPmp,
            Self::Upnp { .. } => Protocol::Upnp,
        }
    }
}

/// Keeps the router's ports open for as long as it runs.
#[derive(Clone)]
pub struct PortMapper {
    status: watch::Receiver<MappingStatus>,
    cancel: CancellationToken,
}

impl PortMapper {
    /// A mapper for `ports` and the future that does its work. Spawn the
    /// future on a Tokio runtime; it runs until [`Self::shutdown`].
    pub fn new(
        config: MapperConfig,
        ports: Vec<u16>,
    ) -> (Self, impl std::future::Future<Output = ()> + Send + 'static) {
        let mut ports = ports;
        ports.sort_unstable();
        ports.dedup();
        let (sender, status) = watch::channel(MappingStatus {
            state: MappingState::Probing,
            protocol: None,
            ports: Vec::new(),
            tried: Vec::new(),
        });
        let cancel = CancellationToken::new();
        let worker = Worker {
            config,
            ports,
            status: sender,
            cancel: cancel.clone(),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(5))
                .no_proxy()
                .build()
                .unwrap_or_default(),
        };
        (Self { status, cancel }, worker.run())
    }

    /// The current status.
    #[must_use]
    pub fn status(&self) -> MappingStatus {
        self.status.borrow().clone()
    }

    /// Wait for the status to change.
    pub async fn changed(&mut self) {
        let _ = self.status.changed().await;
    }

    /// Stop renewing and remove every mapping, waiting at most `limit`.
    pub async fn shutdown(&self, limit: Duration) {
        self.cancel.cancel();
        let mut status = self.status.clone();
        let _ = tokio::time::timeout(
            limit,
            status.wait_for(|status| status.state == MappingState::Stopped),
        )
        .await;
    }
}

struct Worker {
    config: MapperConfig,
    ports: Vec<u16>,
    status: watch::Sender<MappingStatus>,
    cancel: CancellationToken,
    http: reqwest::Client,
}

impl Worker {
    async fn run(self) {
        let worker = Arc::new(self);
        loop {
            worker.publish(MappingState::Probing, None, &[], None);
            let (established, tried) = tokio::select! {
                () = worker.cancel.cancelled() => break,
                result = worker.establish() => result,
            };
            let Some((session, mut leases)) = established else {
                worker.publish(MappingState::Unavailable, None, &[], Some(tried));
                tokio::select! {
                    () = worker.cancel.cancelled() => break,
                    () = tokio::time::sleep(worker.config.retry_after) => continue,
                }
            };
            worker.publish(MappingState::Mapped, Some(&session), &leases, Some(tried));
            loop {
                tokio::select! {
                    () = worker.cancel.cancelled() => {
                        worker.remove(&session, &leases).await;
                        worker.publish(MappingState::Stopped, None, &[], None);
                        return;
                    }
                    () = tokio::time::sleep(renew_after(&leases)) => {}
                }
                match worker.renew(&session, &leases).await {
                    Ok(renewed) => {
                        leases = renewed;
                        worker.publish(MappingState::Mapped, Some(&session), &leases, None);
                    }
                    // The router forgot or changed; start over.
                    Err(_) => break,
                }
            }
        }
        worker.publish(MappingState::Stopped, None, &[], None);
    }

    fn publish(
        &self,
        state: MappingState,
        session: Option<&Session>,
        leases: &[Lease],
        tried: Option<Vec<(Protocol, Option<MapError>)>>,
    ) {
        self.status.send_modify(|status| {
            status.state = state;
            status.protocol = session.map(Session::protocol);
            status.ports = leases
                .iter()
                .map(|lease| MappedPort {
                    internal_port: lease.internal_port,
                    external_port: lease.external_port,
                    transport: lease.transport,
                })
                .collect();
            if let Some(tried) = tried {
                status.tried = tried;
            }
        });
    }

    /// Try each protocol until one opens every port for both transports.
    async fn establish(
        &self,
    ) -> (
        Option<(Session, Vec<Lease>)>,
        Vec<(Protocol, Option<MapError>)>,
    ) {
        let Some(router_ip) = self.config.gateway.or_else(default_gateway) else {
            return (
                None,
                PROTOCOLS
                    .iter()
                    .map(|protocol| (*protocol, Some(MapError::NoGateway)))
                    .collect(),
            );
        };
        let router = SocketAddr::from((router_ip, self.config.pmp_port));
        let client = local_address(router).await;
        let mut tried = Vec::new();
        for protocol in PROTOCOLS {
            let session = match protocol {
                Protocol::Pcp => client.clone().map(|client| Session::Pcp { router, client }),
                Protocol::NatPmp => Ok(Session::NatPmp { router }),
                Protocol::Upnp => match client.clone() {
                    Ok(client) => upnp::discover(
                        self.config.ssdp_target,
                        self.config.discovery_wait,
                        &self.http,
                    )
                    .await
                    .map(|gateway| Session::Upnp { gateway, client }),
                    Err(error) => Err(error),
                },
            };
            let session = match session {
                Ok(session) => session,
                Err(error) => {
                    tried.push((protocol, Some(error)));
                    continue;
                }
            };
            let mut leases = Vec::new();
            let mut failure = None;
            'ports: for &port in &self.ports {
                for transport in TRANSPORTS {
                    let wanted = Lease {
                        transport,
                        internal_port: port,
                        external_port: port,
                        lifetime: self.config.lifetime,
                        nonce: nonce(),
                    };
                    match self.request(&session, wanted).await {
                        Ok(lease) => leases.push(lease),
                        Err(error) => {
                            failure = Some(error);
                            break 'ports;
                        }
                    }
                }
            }
            match failure {
                None => {
                    tried.push((protocol, None));
                    return (Some((session, leases)), tried);
                }
                Some(error) => {
                    // Undo what this protocol opened before trying the next.
                    self.remove(&session, &leases).await;
                    tried.push((protocol, Some(error)));
                }
            }
        }
        (None, tried)
    }

    async fn renew(&self, session: &Session, leases: &[Lease]) -> Result<Vec<Lease>, MapError> {
        let mut renewed = Vec::with_capacity(leases.len());
        for lease in leases {
            let next = self
                .request(
                    session,
                    Lease {
                        lifetime: self.config.lifetime,
                        ..*lease
                    },
                )
                .await?;
            if next.external_port != lease.external_port {
                return Err(MapError::Malformed);
            }
            renewed.push(next);
        }
        Ok(renewed)
    }

    async fn remove(&self, session: &Session, leases: &[Lease]) {
        for lease in leases {
            let _ = match session {
                Session::Upnp { gateway, .. } => {
                    upnp::delete(&self.http, gateway, lease.transport, lease.external_port).await
                }
                _ => self
                    .request(
                        session,
                        Lease {
                            lifetime: 0,
                            ..*lease
                        },
                    )
                    .await
                    .map(drop),
            };
        }
    }

    /// Ask for one mapping (lifetime 0 removes it). The request names the
    /// external port wanted; the reply says which port the router gave.
    async fn request(&self, session: &Session, lease: Lease) -> Result<Lease, MapError> {
        match session {
            Session::Upnp { gateway, client } => {
                let lifetime = upnp::add(
                    &self.http,
                    gateway,
                    *client,
                    lease.transport,
                    lease.internal_port,
                    lease.lifetime,
                    &self.config.description,
                )
                .await?;
                Ok(Lease { lifetime, ..lease })
            }
            Session::NatPmp { router } => {
                let request = wire::nat_pmp_request(
                    lease.transport,
                    lease.internal_port,
                    lease.external_port,
                    lease.lifetime,
                );
                let reply = self
                    .exchange(*router, &request, |bytes| {
                        wire::nat_pmp_response(bytes, lease.transport, lease.internal_port)
                    })
                    .await?;
                granted(reply, lease)
            }
            Session::Pcp { router, client } => {
                let request = wire::pcp_request(
                    *client,
                    lease.nonce,
                    lease.transport,
                    lease.internal_port,
                    lease.external_port,
                    lease.lifetime,
                );
                let reply = self
                    .exchange(*router, &request, |bytes| {
                        wire::pcp_response(bytes, lease.nonce, lease.transport, lease.internal_port)
                    })
                    .await?;
                granted(reply, lease)
            }
        }
    }

    /// Send `request` to the router and wait for a matching reply, resending
    /// after `reply_timeout`, then twice as long, then twice again. Replies
    /// from any other address are ignored.
    async fn exchange(
        &self,
        router: SocketAddr,
        request: &[u8],
        parse: impl Fn(&[u8]) -> Option<Reply>,
    ) -> Result<Reply, MapError> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
        let mut wait = self.config.reply_timeout;
        let mut buffer = [0_u8; 1100];
        for _ in 0..3 {
            socket.send_to(request, router).await?;
            let deadline = tokio::time::Instant::now() + wait;
            loop {
                let Ok(received) =
                    tokio::time::timeout_at(deadline, socket.recv_from(&mut buffer)).await
                else {
                    break;
                };
                let (length, from) = received?;
                if from != router {
                    continue;
                }
                if let Some(reply) = parse(&buffer[..length]) {
                    return Ok(reply);
                }
            }
            wait *= 2;
        }
        Err(MapError::NoAnswer)
    }
}

fn granted(reply: Reply, lease: Lease) -> Result<Lease, MapError> {
    match reply {
        Reply::Granted(granted) => Ok(Lease {
            external_port: if lease.lifetime == 0 {
                lease.external_port
            } else {
                granted.external_port
            },
            lifetime: granted.lifetime,
            ..lease
        }),
        Reply::Refused(code) => Err(MapError::Refused(u32::from(code))),
        Reply::UnsupportedVersion => Err(MapError::Unsupported),
    }
}

/// Renew at half the shortest lifetime; permanent mappings are checked
/// every half hour.
fn renew_after(leases: &[Lease]) -> Duration {
    let seconds = leases
        .iter()
        .map(|lease| {
            if lease.lifetime == 0 {
                3600
            } else {
                lease.lifetime
            }
        })
        .min()
        .unwrap_or(3600);
    Duration::from_millis(u64::from(seconds) * 500).max(Duration::from_millis(500))
}

fn nonce() -> [u8; 12] {
    let mut nonce = [0_u8; 12];
    // An all-zero nonce still works; the router just cannot tell clients apart.
    let _ = getrandom::fill(&mut nonce);
    nonce
}

/// The address this computer uses to reach the router.
async fn local_address(router: SocketAddr) -> Result<Ipv4Addr, MapError> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    socket.connect(router).await?;
    match socket.local_addr()? {
        SocketAddr::V4(address) => Ok(*address.ip()),
        SocketAddr::V6(_) => Err(MapError::NoGateway),
    }
}

/// The IPv4 default gateway, from the operating system's routing table.
#[must_use]
pub fn default_gateway() -> Option<Ipv4Addr> {
    netdev::get_default_gateway()
        .ok()?
        .ipv4
        .into_iter()
        .find(|address| !address.is_unspecified())
}
