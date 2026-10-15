//! NAT-PMP (RFC 6886) and PCP (RFC 6887) MAP messages. Both speak to the
//! default gateway on UDP port 5351. Responses are untrusted and checked
//! field by field before use.

use std::net::{Ipv4Addr, Ipv6Addr};

use crate::Transport;

/// Port the gateway answers NAT-PMP and PCP on.
pub const SERVER_PORT: u16 = 5351;

/// A granted mapping, as the gateway reported it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Granted {
    /// Port peers reach on the router.
    pub external_port: u16,
    /// Seconds the mapping lasts.
    pub lifetime: u32,
}

/// How a gateway answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// The mapping was created, renewed, or (with lifetime 0) removed.
    Granted(Granted),
    /// The gateway refused with this result code.
    Refused(u16),
    /// The gateway speaks another protocol version; try the next protocol.
    UnsupportedVersion,
}

const NAT_PMP_VERSION: u8 = 0;
const PCP_VERSION: u8 = 2;
const PCP_MAP: u8 = 1;
/// A PCP request or response with a MAP payload.
pub const PCP_MAP_LENGTH: usize = 60;
const PCP_UNSUPPORTED_VERSION: u8 = 1;

const fn nat_pmp_opcode(transport: Transport) -> u8 {
    match transport {
        Transport::Udp => 1,
        Transport::Tcp => 2,
    }
}

const fn ip_protocol(transport: Transport) -> u8 {
    match transport {
        Transport::Tcp => 6,
        Transport::Udp => 17,
    }
}

/// A NAT-PMP mapping request; lifetime 0 removes the mapping.
#[must_use]
pub fn nat_pmp_request(
    transport: Transport,
    internal_port: u16,
    external_port: u16,
    lifetime: u32,
) -> [u8; 12] {
    let mut request = [0_u8; 12];
    request[0] = NAT_PMP_VERSION;
    request[1] = nat_pmp_opcode(transport);
    request[4..6].copy_from_slice(&internal_port.to_be_bytes());
    request[6..8].copy_from_slice(&external_port.to_be_bytes());
    request[8..12].copy_from_slice(&lifetime.to_be_bytes());
    request
}

/// Read a NAT-PMP mapping response to a request for `transport` and
/// `internal_port`. `None` when it answers some other request.
#[must_use]
pub fn nat_pmp_response(bytes: &[u8], transport: Transport, internal_port: u16) -> Option<Reply> {
    let [version, opcode, result_high, result_low, ..] = *bytes else {
        return None;
    };
    if version != NAT_PMP_VERSION {
        return Some(Reply::UnsupportedVersion);
    }
    if opcode != 128 + nat_pmp_opcode(transport) {
        return None;
    }
    let result = u16::from_be_bytes([result_high, result_low]);
    if result != 0 {
        return Some(Reply::Refused(result));
    }
    let body: &[u8; 16] = bytes.get(..16)?.try_into().ok()?;
    if u16::from_be_bytes([body[8], body[9]]) != internal_port {
        return None;
    }
    Some(Reply::Granted(Granted {
        external_port: u16::from_be_bytes([body[10], body[11]]),
        lifetime: u32::from_be_bytes([body[12], body[13], body[14], body[15]]),
    }))
}

/// A PCP MAP request from `client`; lifetime 0 removes the mapping. Renewals
/// and removals reuse the nonce of the original request.
#[must_use]
pub fn pcp_request(
    client: Ipv4Addr,
    nonce: [u8; 12],
    transport: Transport,
    internal_port: u16,
    external_port: u16,
    lifetime: u32,
) -> [u8; PCP_MAP_LENGTH] {
    let mut request = [0_u8; PCP_MAP_LENGTH];
    request[0] = PCP_VERSION;
    request[1] = PCP_MAP;
    request[4..8].copy_from_slice(&lifetime.to_be_bytes());
    request[8..24].copy_from_slice(&client.to_ipv6_mapped().octets());
    request[24..36].copy_from_slice(&nonce);
    request[36] = ip_protocol(transport);
    request[40..42].copy_from_slice(&internal_port.to_be_bytes());
    request[42..44].copy_from_slice(&external_port.to_be_bytes());
    // No preferred external address: the IPv4-mapped all-zeros address.
    request[44..60].copy_from_slice(&Ipv4Addr::UNSPECIFIED.to_ipv6_mapped().octets());
    request
}

/// Read a PCP MAP response to the request with `nonce`. A NAT-PMP-only
/// gateway answers with version 0, reported as unsupported. `None` when it
/// answers some other request.
#[must_use]
pub fn pcp_response(
    bytes: &[u8],
    nonce: [u8; 12],
    transport: Transport,
    internal_port: u16,
) -> Option<Reply> {
    let [version, opcode, _, result, ..] = *bytes else {
        return None;
    };
    if version != PCP_VERSION {
        return Some(Reply::UnsupportedVersion);
    }
    if opcode != 0x80 | PCP_MAP {
        return None;
    }
    if result == PCP_UNSUPPORTED_VERSION {
        return Some(Reply::UnsupportedVersion);
    }
    // Error responses may be short; a successful MAP response is not.
    if result != 0 {
        return Some(Reply::Refused(u16::from(result)));
    }
    let body: &[u8; PCP_MAP_LENGTH] = bytes.get(..PCP_MAP_LENGTH)?.try_into().ok()?;
    if body[24..36] != nonce
        || body[36] != ip_protocol(transport)
        || u16::from_be_bytes([body[40], body[41]]) != internal_port
    {
        return None;
    }
    Some(Reply::Granted(Granted {
        external_port: u16::from_be_bytes([body[42], body[43]]),
        lifetime: u32::from_be_bytes([body[4], body[5], body[6], body[7]]),
    }))
}

/// The IPv4 client address a PCP request carries, for test servers.
#[must_use]
pub fn pcp_client(request: &[u8; PCP_MAP_LENGTH]) -> Option<Ipv4Addr> {
    let octets: [u8; 16] = request[8..24].try_into().ok()?;
    Ipv6Addr::from(octets).to_ipv4_mapped()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nat_pmp_round_trips_and_rejects_mismatches() {
        let request = nat_pmp_request(Transport::Tcp, 51413, 51413, 7200);
        assert_eq!(
            request,
            [0, 2, 0, 0, 0xC8, 0xD5, 0xC8, 0xD5, 0, 0, 0x1C, 0x20]
        );
        let mut response = [0_u8; 16];
        response[1] = 130;
        response[8..10].copy_from_slice(&51413_u16.to_be_bytes());
        response[10..12].copy_from_slice(&60000_u16.to_be_bytes());
        response[12..16].copy_from_slice(&3600_u32.to_be_bytes());
        assert_eq!(
            nat_pmp_response(&response, Transport::Tcp, 51413),
            Some(Reply::Granted(Granted {
                external_port: 60000,
                lifetime: 3600
            }))
        );
        // The UDP answer, another port, a short packet: not this request.
        assert_eq!(nat_pmp_response(&response, Transport::Udp, 51413), None);
        assert_eq!(nat_pmp_response(&response, Transport::Tcp, 6881), None);
        assert_eq!(
            nat_pmp_response(&response[..9], Transport::Tcp, 51413),
            None
        );
        response[3] = 2;
        assert_eq!(
            nat_pmp_response(&response, Transport::Tcp, 51413),
            Some(Reply::Refused(2))
        );
        response[0] = 2;
        assert_eq!(
            nat_pmp_response(&response, Transport::Tcp, 51413),
            Some(Reply::UnsupportedVersion)
        );
    }

    #[test]
    fn pcp_map_round_trips_and_checks_the_nonce() {
        let nonce = [7_u8; 12];
        let client = Ipv4Addr::new(192, 168, 1, 20);
        let request = pcp_request(client, nonce, Transport::Udp, 51413, 51413, 7200);
        assert_eq!(&request[..2], &[2, 1]);
        assert_eq!(pcp_client(&request), Some(client));
        assert_eq!(request[36], 17);

        let mut response = request;
        response[1] = 0x81;
        response[4..8].copy_from_slice(&1800_u32.to_be_bytes());
        response[42..44].copy_from_slice(&40000_u16.to_be_bytes());
        assert_eq!(
            pcp_response(&response, nonce, Transport::Udp, 51413),
            Some(Reply::Granted(Granted {
                external_port: 40000,
                lifetime: 1800
            }))
        );
        assert_eq!(
            pcp_response(&response, [8; 12], Transport::Udp, 51413),
            None
        );
        assert_eq!(pcp_response(&response, nonce, Transport::Tcp, 51413), None);
        response[3] = 2;
        assert_eq!(
            pcp_response(&response[..24], nonce, Transport::Udp, 51413),
            Some(Reply::Refused(2))
        );
        response[3] = 1;
        assert_eq!(
            pcp_response(&response, nonce, Transport::Udp, 51413),
            Some(Reply::UnsupportedVersion)
        );
        // A NAT-PMP gateway answering a PCP request.
        assert_eq!(
            pcp_response(&[0, 129, 0, 1, 0, 0, 0, 0], nonce, Transport::Udp, 51413),
            Some(Reply::UnsupportedVersion)
        );
    }
}
