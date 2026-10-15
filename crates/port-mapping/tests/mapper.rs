//! The mapper against fake PCP, NAT-PMP, and `UPnP` gateways on loopback.

use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

use port_mapping::{
    MapError, MapperConfig, MappingState, MappingStatus, PortMapper, Protocol, Transport, wire,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, UdpSocket},
};

/// How a fake NAT-PMP/PCP gateway behaves.
#[derive(Clone, Copy)]
enum Pmp {
    /// Speaks PCP and grants this lifetime.
    Pcp { lifetime: u32 },
    /// Answers PCP as an old NAT-PMP-only router; grants NAT-PMP.
    NatPmpOnly { lifetime: u32 },
    /// Speaks PCP but refuses UDP mappings.
    PcpWithoutUdp,
    /// Never answers.
    Silent,
}

/// One request a fake gateway received: protocol, transport, port, lifetime, nonce.
type Seen = (Protocol, Transport, u16, u32, [u8; 12]);

async fn pmp_gateway(mode: Pmp) -> (u16, Arc<Mutex<Vec<Seen>>>) {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = socket.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        let mut buffer = [0_u8; 1100];
        loop {
            let (length, from) = socket.recv_from(&mut buffer).await.unwrap();
            let request = &buffer[..length];
            if let Some(reply) = answer(mode, request, &log) {
                socket.send_to(&reply, from).await.unwrap();
            }
        }
    });
    (port, seen)
}

fn answer(mode: Pmp, request: &[u8], log: &Mutex<Vec<Seen>>) -> Option<Vec<u8>> {
    if matches!(mode, Pmp::Silent) {
        return None;
    }
    let transport = |code: u8, udp: u8| {
        if code == udp {
            Transport::Udp
        } else {
            Transport::Tcp
        }
    };
    match request[0] {
        2 => {
            let request: [u8; wire::PCP_MAP_LENGTH] = request.try_into().ok()?;
            assert_eq!(wire::pcp_client(&request), Some(Ipv4Addr::LOCALHOST));
            let transport = transport(request[36], 17);
            let port = u16::from_be_bytes([request[40], request[41]]);
            let lifetime = u32::from_be_bytes([request[4], request[5], request[6], request[7]]);
            let nonce: [u8; 12] = request[24..36].try_into().unwrap();
            let granted = match mode {
                Pmp::NatPmpOnly { .. } => {
                    // RFC 6887 §9: a NAT-PMP server answers version 0, result 1.
                    return Some(vec![0, 129, 0, 1, 0, 0, 0, 0]);
                }
                Pmp::PcpWithoutUdp if transport == Transport::Udp && lifetime > 0 => {
                    let mut refused = request.to_vec();
                    refused[1] = 0x81;
                    refused[3] = 8; // NO_RESOURCES
                    return Some(refused);
                }
                Pmp::Pcp { lifetime: grant } => grant,
                _ => 120,
            };
            log.lock()
                .unwrap()
                .push((Protocol::Pcp, transport, port, lifetime, nonce));
            let mut reply = request.to_vec();
            reply[1] = 0x81;
            reply[3] = 0;
            reply[4..8].copy_from_slice(&if lifetime == 0 { 0 } else { granted }.to_be_bytes());
            Some(reply)
        }
        0 => {
            let Pmp::NatPmpOnly { lifetime: grant } = mode else {
                return None;
            };
            let transport = transport(request[1], 1);
            let port = u16::from_be_bytes([request[4], request[5]]);
            let lifetime = u32::from_be_bytes([request[8], request[9], request[10], request[11]]);
            log.lock()
                .unwrap()
                .push((Protocol::NatPmp, transport, port, lifetime, [0; 12]));
            let mut reply = vec![0_u8; 16];
            reply[1] = 128 + request[1];
            reply[8..10].copy_from_slice(&port.to_be_bytes());
            reply[10..12].copy_from_slice(&port.to_be_bytes());
            reply[12..16].copy_from_slice(&if lifetime == 0 { 0 } else { grant }.to_be_bytes());
            Some(reply)
        }
        _ => None,
    }
}

/// A fake `UPnP` gateway: an SSDP responder and an HTTP server for the
/// description and SOAP control. It keeps only permanent leases.
async fn igd() -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let http = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let http_port = http.local_addr().unwrap().port();
    let ssdp = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let ssdp_address = ssdp.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buffer = [0_u8; 1024];
        loop {
            let (length, from) = ssdp.recv_from(&mut buffer).await.unwrap();
            if buffer[..length].starts_with(b"M-SEARCH") {
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\nLOCATION: http://127.0.0.1:{http_port}/rootDesc.xml\r\n\r\n"
                );
                ssdp.send_to(reply.as_bytes(), from).await.unwrap();
            }
        }
    });
    let actions = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&actions);
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = http.accept().await.unwrap();
            let log = Arc::clone(&log);
            tokio::spawn(async move {
                let request = read_request(&mut stream).await;
                let (status, body) = if request.starts_with("GET /rootDesc.xml") {
                    ("200 OK", "<root><device><deviceList><device><deviceList><device><serviceList><service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType><controlURL>/ctl/IPConn</controlURL></service></serviceList></device></deviceList></device></deviceList></device></root>".to_owned())
                } else if request.starts_with("POST /ctl/IPConn") {
                    let field = |name: &str| {
                        request
                            .split(&format!("<{name}>"))
                            .nth(1)
                            .and_then(|rest| rest.split('<').next())
                            .unwrap_or("")
                            .to_owned()
                    };
                    let action = if request.contains("#AddPortMapping") {
                        "add"
                    } else {
                        "delete"
                    };
                    let lease = field("NewLeaseDuration");
                    if action == "add" && lease != "0" {
                        ("500 Internal Server Error", "<s:Envelope><s:Body><s:Fault><detail><UPnPError><errorCode>725</errorCode></UPnPError></detail></s:Fault></s:Body></s:Envelope>".to_owned())
                    } else {
                        assert!(action == "delete" || field("NewInternalClient") == "127.0.0.1");
                        log.lock().unwrap().push(format!(
                            "{action} {} {}",
                            field("NewProtocol"),
                            field("NewExternalPort")
                        ));
                        ("200 OK", "<s:Envelope><s:Body/></s:Envelope>".to_owned())
                    }
                } else {
                    ("404 Not Found", String::new())
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    (ssdp_address, actions)
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).await.unwrap();
        data.extend_from_slice(&buffer[..read]);
        let text = String::from_utf8_lossy(&data).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let length = head
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if body.len() >= length {
                return text;
            }
        }
        if read == 0 {
            return text;
        }
    }
}

fn config(pmp_port: u16, ssdp_target: SocketAddr) -> MapperConfig {
    MapperConfig {
        gateway: Some(Ipv4Addr::LOCALHOST),
        pmp_port,
        ssdp_target,
        reply_timeout: Duration::from_millis(40),
        discovery_wait: Duration::from_millis(400),
        retry_after: Duration::from_secs(60),
        ..MapperConfig::default()
    }
}

/// An address where nothing answers SSDP.
async fn no_ssdp() -> SocketAddr {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let _keep = socket;
        std::future::pending::<()>().await;
    });
    address
}

async fn settled(mapper: &mut PortMapper) -> MappingStatus {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let status = mapper.status();
            if status.state != MappingState::Probing {
                return status;
            }
            mapper.changed().await;
        }
    })
    .await
    .expect("the mapper settles")
}

#[tokio::test]
async fn pcp_opens_tcp_and_udp_renews_at_half_life_and_removes_at_shutdown() {
    let (pmp_port, seen) = pmp_gateway(Pmp::Pcp { lifetime: 2 }).await;
    let (mut mapper, run) =
        PortMapper::new(config(pmp_port, no_ssdp().await), vec![51413, 40000, 51413]);
    tokio::spawn(run);
    let status = settled(&mut mapper).await;
    assert_eq!(status.state, MappingState::Mapped);
    assert_eq!(status.protocol, Some(Protocol::Pcp));
    assert_eq!(status.tried, [(Protocol::Pcp, None)]);
    let mut ports: Vec<(u16, Transport)> = status
        .ports
        .iter()
        .map(|port| (port.internal_port, port.transport))
        .collect();
    ports.sort_by_key(|(port, transport)| (*port, *transport == Transport::Udp));
    assert_eq!(
        ports,
        [
            (40000, Transport::Tcp),
            (40000, Transport::Udp),
            (51413, Transport::Tcp),
            (51413, Transport::Udp)
        ]
    );

    // Lifetime 2 s: renewed after about one second, with the same nonces.
    tokio::time::sleep(Duration::from_millis(1600)).await;
    let requests = seen.lock().unwrap().clone();
    assert!(requests.len() >= 8, "{requests:?}");
    for first in &requests[..4] {
        assert!(
            requests[4..].iter().any(|renewal| renewal.4 == first.4
                && renewal.2 == first.2
                && renewal.1 == first.1),
            "renewal of {first:?}"
        );
    }

    mapper.shutdown(Duration::from_secs(5)).await;
    assert_eq!(mapper.status().state, MappingState::Stopped);
    let removals: Vec<Seen> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|seen| seen.3 == 0)
        .copied()
        .collect();
    assert_eq!(removals.len(), 4);
    assert!(
        removals
            .iter()
            .all(|removal| requests[..4].iter().any(|first| first.4 == removal.4))
    );
}

#[tokio::test]
async fn a_nat_pmp_router_is_used_when_pcp_is_unsupported() {
    let (pmp_port, seen) = pmp_gateway(Pmp::NatPmpOnly { lifetime: 3600 }).await;
    let (mut mapper, run) = PortMapper::new(config(pmp_port, no_ssdp().await), vec![51413]);
    tokio::spawn(run);
    let status = settled(&mut mapper).await;
    assert_eq!(status.protocol, Some(Protocol::NatPmp));
    assert_eq!(
        status.tried,
        [
            (Protocol::Pcp, Some(MapError::Unsupported)),
            (Protocol::NatPmp, None)
        ]
    );
    mapper.shutdown(Duration::from_secs(5)).await;
    let requests = seen.lock().unwrap().clone();
    let transports = |lifetime: fn(u32) -> bool| {
        requests
            .iter()
            .filter(|seen| lifetime(seen.3))
            .map(|seen| seen.1)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        transports(|lifetime| lifetime > 0),
        [Transport::Tcp, Transport::Udp]
    );
    assert_eq!(
        transports(|lifetime| lifetime == 0),
        [Transport::Tcp, Transport::Udp]
    );
}

#[tokio::test]
async fn a_partial_pcp_mapping_is_undone_before_the_next_protocol() {
    let (pmp_port, seen) = pmp_gateway(Pmp::PcpWithoutUdp).await;
    let (ssdp, actions) = igd().await;
    let (mut mapper, run) = PortMapper::new(config(pmp_port, ssdp), vec![51413]);
    tokio::spawn(run);
    let status = settled(&mut mapper).await;
    assert_eq!(status.protocol, Some(Protocol::Upnp));
    assert_eq!(status.tried[0], (Protocol::Pcp, Some(MapError::Refused(8))));
    // The TCP mapping PCP granted was removed again.
    let pcp: Vec<(Transport, u32)> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|seen| (seen.1, seen.3))
        .collect();
    assert_eq!(pcp, [(Transport::Tcp, 7200), (Transport::Tcp, 0)]);
    mapper.shutdown(Duration::from_secs(5)).await;
    assert_eq!(actions.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn upnp_maps_permanently_when_the_router_refuses_leases_and_removes_at_shutdown() {
    let silent = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let (ssdp, actions) = igd().await;
    let (mut mapper, run) = PortMapper::new(
        config(silent.local_addr().unwrap().port(), ssdp),
        vec![51413],
    );
    tokio::spawn(run);
    let status = settled(&mut mapper).await;
    assert_eq!(status.state, MappingState::Mapped);
    assert_eq!(status.protocol, Some(Protocol::Upnp));
    assert_eq!(
        status.tried,
        [
            (Protocol::Pcp, Some(MapError::NoAnswer)),
            (Protocol::NatPmp, Some(MapError::NoAnswer)),
            (Protocol::Upnp, None)
        ]
    );
    assert_eq!(status.ports.len(), 2);
    mapper.shutdown(Duration::from_secs(5)).await;
    assert_eq!(
        *actions.lock().unwrap(),
        [
            "add TCP 51413",
            "add UDP 51413",
            "delete TCP 51413",
            "delete UDP 51413"
        ]
    );
    drop(silent);
}

#[tokio::test]
async fn nothing_answering_is_reported_per_protocol() {
    let (pmp_port, _) = pmp_gateway(Pmp::Silent).await;
    let (mut mapper, run) = PortMapper::new(config(pmp_port, no_ssdp().await), vec![51413]);
    tokio::spawn(run);
    let status = settled(&mut mapper).await;
    assert_eq!(status.state, MappingState::Unavailable);
    assert_eq!(status.protocol, None);
    assert_eq!(status.ports, []);
    assert_eq!(
        status.tried,
        [
            (Protocol::Pcp, Some(MapError::NoAnswer)),
            (Protocol::NatPmp, Some(MapError::NoAnswer)),
            (Protocol::Upnp, Some(MapError::NoAnswer))
        ]
    );
    mapper.shutdown(Duration::from_secs(5)).await;
    assert_eq!(mapper.status().state, MappingState::Stopped);
}
