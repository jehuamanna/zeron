//! `UPnP` Internet Gateway Device port mapping: SSDP discovery, the device
//! description, then SOAP `AddPortMapping` and `DeletePortMapping`.
//!
//! Everything a router sends is untrusted: responses are size-limited, the
//! description and control URLs must be plain HTTP on the address that
//! answered discovery, and redirects are never followed.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use reqwest::Url;
use tokio::net::UdpSocket;

use crate::{MapError, Transport};

/// Where SSDP discovery is sent.
pub const SSDP_MULTICAST: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)), 1900);
/// Largest description or SOAP response read.
const MAX_BODY_BYTES: usize = 64 * 1024;
/// SOAP error a router returns when it only keeps permanent mappings.
const ONLY_PERMANENT_LEASES: u32 = 725;

const SEARCH_TARGETS: [&str; 2] = [
    "urn:schemas-upnp-org:device:InternetGatewayDevice:2",
    "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
];
const SERVICE_TYPES: [&str; 3] = [
    "urn:schemas-upnp-org:service:WANIPConnection:2",
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
];

/// A gateway's port-mapping service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gateway {
    /// Where SOAP requests go.
    pub control_url: Url,
    /// The exact service type the gateway advertised.
    pub service_type: String,
}

fn search_request(target: &str) -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {target}\r\n\r\n"
    )
}

/// The description URL in an SSDP response from `responder`. Only plain
/// HTTP on the responder's own address is accepted, so a reply cannot send
/// Zeron to another host.
#[must_use]
pub fn location(response: &[u8], responder: IpAddr) -> Option<Url> {
    let text = std::str::from_utf8(response).ok()?;
    let mut lines = text.split("\r\n");
    if !lines.next()?.starts_with("HTTP/1.1 200") {
        return None;
    }
    let value = lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("location")
            .then(|| value.trim())
    })?;
    let url = Url::parse(value).ok()?;
    same_host(&url, responder).then_some(url)
}

fn same_host(url: &Url, host: IpAddr) -> bool {
    url.scheme() == "http"
        && url.username().is_empty()
        && url.password().is_none()
        && match url.host() {
            Some(url::Host::Ipv4(address)) => IpAddr::V4(address) == host,
            _ => false,
        }
}

/// The port-mapping service in a device description fetched from
/// `location`, preferring `WANIPConnection` over `WANPPPConnection`.
#[must_use]
pub fn gateway(description: &str, location: &Url, responder: IpAddr) -> Option<Gateway> {
    let base = element(description, "URLBase")
        .and_then(|base| Url::parse(base.trim()).ok())
        .unwrap_or_else(|| location.clone());
    let services: Vec<(&str, &str)> = blocks(description, "service")
        .filter_map(|service| {
            Some((
                element(service, "serviceType")?.trim(),
                element(service, "controlURL")?.trim(),
            ))
        })
        .collect();
    SERVICE_TYPES.iter().find_map(|wanted| {
        let (service_type, control) = services.iter().find(|(kind, _)| kind == wanted)?;
        let control_url = base.join(control).ok()?;
        same_host(&control_url, responder).then(|| Gateway {
            control_url,
            service_type: (*service_type).to_owned(),
        })
    })
}

/// The text of the first `<name>` element, namespace prefixes allowed.
fn element<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    blocks(xml, name).next()
}

/// The contents of every `<name>…</name>` element, in order.
fn blocks<'a>(xml: &'a str, name: &str) -> impl Iterator<Item = &'a str> + use<'a> {
    let name = name.to_owned();
    let mut rest = xml;
    std::iter::from_fn(move || {
        loop {
            let start = rest.find('<')?;
            rest = &rest[start + 1..];
            let end = rest.find('>')?;
            let tag = &rest[..end];
            rest = &rest[end + 1..];
            let local = tag.rsplit(':').next().unwrap_or(tag);
            if tag.starts_with('/') || local.trim_end_matches('/') != name.as_str() {
                continue;
            }
            let close = rest.find(&format!("/{}>", tag.split_whitespace().next()?))?;
            let inner = rest[..close].strip_suffix('<')?;
            rest = &rest[close..];
            return Some(inner);
        }
    })
}

fn envelope(service_type: &str, action: &str, arguments: &[(&str, String)]) -> String {
    let arguments = arguments
        .iter()
        .fold(String::new(), |mut body, (name, value)| {
            use std::fmt::Write as _;
            let _ = write!(body, "<{name}>{value}</{name}>");
            body
        });
    format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{action} xmlns:u=\"{service_type}\">{arguments}</u:{action}></s:Body></s:Envelope>"
    )
}

const fn protocol_name(transport: Transport) -> &'static str {
    match transport {
        Transport::Tcp => "TCP",
        Transport::Udp => "UDP",
    }
}

/// Find the gateway's port-mapping service. SSDP goes to `target`, the
/// multicast group outside tests.
///
/// # Errors
///
/// [`MapError::NoAnswer`] when no gateway answered within `wait`.
pub async fn discover(
    target: SocketAddr,
    wait: Duration,
    http: &reqwest::Client,
) -> Result<Gateway, MapError> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    for search in SEARCH_TARGETS {
        socket
            .send_to(search_request(search).as_bytes(), target)
            .await?;
    }
    let deadline = tokio::time::Instant::now() + wait;
    let mut buffer = [0_u8; 2048];
    loop {
        let Ok(received) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buffer)).await
        else {
            return Err(MapError::NoAnswer);
        };
        let (length, responder) = received?;
        let Some(location) = location(&buffer[..length], responder.ip()) else {
            continue;
        };
        let Ok(description) = fetch(http.get(location.clone())).await else {
            continue;
        };
        if let Some(gateway) = gateway(&description, &location, responder.ip()) {
            return Ok(gateway);
        }
    }
}

async fn fetch(request: reqwest::RequestBuilder) -> Result<String, MapError> {
    let mut response = request.send().await.map_err(|_| MapError::NoAnswer)?;
    let status = response.status();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| MapError::Malformed)? {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(MapError::Malformed);
        }
        body.extend_from_slice(&chunk);
    }
    let text = String::from_utf8(body).map_err(|_| MapError::Malformed)?;
    if status.is_success() {
        Ok(text)
    } else {
        let code = element(&text, "errorCode")
            .and_then(|code| code.trim().parse().ok())
            .unwrap_or(u32::from(status.as_u16()));
        Err(MapError::Refused(code))
    }
}

async fn soap(
    http: &reqwest::Client,
    gateway: &Gateway,
    action: &str,
    arguments: &[(&str, String)],
) -> Result<String, MapError> {
    let request = http
        .post(gateway.control_url.clone())
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header(
            "SOAPAction",
            format!("\"{}#{action}\"", gateway.service_type),
        )
        .body(envelope(&gateway.service_type, action, arguments));
    fetch(request).await
}

/// Map `port` to this computer at `client` for `lease` seconds, falling
/// back to a permanent mapping on routers that keep only those. Returns the
/// lease granted; 0 means permanent.
///
/// # Errors
///
/// The router's refusal or a transport failure.
pub async fn add(
    http: &reqwest::Client,
    gateway: &Gateway,
    client: Ipv4Addr,
    transport: Transport,
    port: u16,
    lease: u32,
    description: &str,
) -> Result<u32, MapError> {
    let arguments = |lease: u32| {
        vec![
            ("NewRemoteHost", String::new()),
            ("NewExternalPort", port.to_string()),
            ("NewProtocol", protocol_name(transport).to_owned()),
            ("NewInternalPort", port.to_string()),
            ("NewInternalClient", client.to_string()),
            ("NewEnabled", "1".to_owned()),
            ("NewPortMappingDescription", description.to_owned()),
            ("NewLeaseDuration", lease.to_string()),
        ]
    };
    match soap(http, gateway, "AddPortMapping", &arguments(lease)).await {
        Ok(_) => Ok(lease),
        Err(MapError::Refused(ONLY_PERMANENT_LEASES)) if lease != 0 => {
            soap(http, gateway, "AddPortMapping", &arguments(0)).await?;
            Ok(0)
        }
        Err(error) => Err(error),
    }
}

/// Remove the mapping of `port`.
///
/// # Errors
///
/// The router's refusal or a transport failure.
pub async fn delete(
    http: &reqwest::Client,
    gateway: &Gateway,
    transport: Transport,
    port: u16,
) -> Result<(), MapError> {
    soap(
        http,
        gateway,
        "DeletePortMapping",
        &[
            ("NewRemoteHost", String::new()),
            ("NewExternalPort", port.to_string()),
            ("NewProtocol", protocol_name(transport).to_owned()),
        ],
    )
    .await
    .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTER: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

    #[test]
    fn locations_must_point_back_at_the_responder() {
        let reply = |location: &str| {
            format!(
                "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=120\r\nLocation: {location}\r\nST: x\r\n\r\n"
            )
        };
        assert_eq!(
            location(
                reply("http://192.168.1.1:5000/rootDesc.xml").as_bytes(),
                ROUTER
            )
            .unwrap()
            .as_str(),
            "http://192.168.1.1:5000/rootDesc.xml"
        );
        for rejected in [
            "http://192.168.1.2:5000/rootDesc.xml",
            "https://192.168.1.1/rootDesc.xml",
            "http://router.example/rootDesc.xml",
            "http://user@192.168.1.1/rootDesc.xml",
            "file:///etc/passwd",
        ] {
            assert_eq!(
                location(reply(rejected).as_bytes(), ROUTER),
                None,
                "{rejected}"
            );
        }
        assert_eq!(location(b"NOTIFY * HTTP/1.1\r\n\r\n", ROUTER), None);
    }

    #[test]
    fn the_wan_ip_service_is_found_and_resolved_against_the_base() {
        let description = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0"><device><serviceList>
<service><serviceType>urn:schemas-upnp-org:service:Layer3Forwarding:1</serviceType><controlURL>/ctl/L3F</controlURL></service>
</serviceList><deviceList><device><deviceList><device><serviceList>
<service><serviceType>urn:schemas-upnp-org:service:WANPPPConnection:1</serviceType><controlURL>/ctl/PPP</controlURL></service>
<service>
  <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
  <controlURL>/ctl/IPConn</controlURL>
</service>
</serviceList></device></deviceList></device></deviceList></device></root>"#;
        let location = Url::parse("http://192.168.1.1:5000/rootDesc.xml").unwrap();
        let found = gateway(description, &location, ROUTER).unwrap();
        assert_eq!(
            found.control_url.as_str(),
            "http://192.168.1.1:5000/ctl/IPConn"
        );
        assert_eq!(
            found.service_type,
            "urn:schemas-upnp-org:service:WANIPConnection:1"
        );
        // A control URL on another host is never used.
        let elsewhere = description
            .replace("/ctl/IPConn", "http://10.0.0.9/ctl")
            .replace("/ctl/PPP", "http://10.0.0.9/ppp");
        assert_eq!(gateway(&elsewhere, &location, ROUTER), None);
        assert_eq!(gateway("<root/>", &location, ROUTER), None);
    }

    #[test]
    fn soap_errors_carry_the_upnp_code() {
        let fault = "<s:Envelope><s:Body><s:Fault><detail><UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\"><errorCode>725</errorCode></UPnPError></detail></s:Fault></s:Body></s:Envelope>";
        assert_eq!(element(fault, "errorCode"), Some("725"));
        let body = envelope(
            "urn:x",
            "DeletePortMapping",
            &[("NewExternalPort", "51413".to_owned())],
        );
        assert!(body.contains("<u:DeletePortMapping xmlns:u=\"urn:x\"><NewExternalPort>51413</NewExternalPort></u:DeletePortMapping>"));
    }
}
