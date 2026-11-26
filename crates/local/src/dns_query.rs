use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

use hickory_proto::op::{Message, Query, ResponseCode};
use hickory_proto::rr::{Name, RData, RecordType};
use tracing::debug;

/// What a resolver said about one name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsAnswer {
    /// It answered with these IPv4 addresses.
    Addresses(Vec<Ipv4Addr>),
    /// It answered with this code and no address: `NXDOMAIN`, `REFUSED`, `NOERROR` (no data).
    Code(String),
    /// It did not answer in time — the query was dropped, or nothing listens.
    Silent,
}

impl DnsAnswer {
    /// Asks the resolver at `server` for `name`'s `A` record directly, bypassing the system's cache.
    #[must_use]
    pub fn ask(server: SocketAddr, name: &str, wait: Duration) -> Self {
        match exchange(server, name, wait) {
            Ok(answer) => answer,
            Err(why) => {
                debug!(%server, name, %why, "local.dns: no answer");
                Self::Silent
            }
        }
    }
}

fn exchange(server: SocketAddr, name: &str, wait: Duration) -> Result<DnsAnswer, String> {
    let name = Name::from_ascii(name).map_err(|error| error.to_string())?;
    let mut query = Message::query();
    query.metadata.recursion_desired = true;
    query.add_query(Query::query(name, RecordType::A));
    let id = query.metadata.id;
    let bytes = query.to_vec().map_err(|error| error.to_string())?;
    let local: IpAddr = if server.ip().is_loopback() {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    };
    let socket = UdpSocket::bind(SocketAddr::new(local, 0)).map_err(|error| error.to_string())?;
    socket
        .set_read_timeout(Some(wait))
        .map_err(|error| error.to_string())?;
    socket
        .send_to(&bytes, server)
        .map_err(|error| error.to_string())?;
    let mut buffer = [0_u8; 4096];
    loop {
        let (read, from) = socket
            .recv_from(&mut buffer)
            .map_err(|error| error.to_string())?;
        if from != server {
            continue;
        }
        let response = Message::from_vec(&buffer[..read]).map_err(|error| error.to_string())?;
        if response.metadata.id != id {
            continue;
        }
        let addresses: Vec<Ipv4Addr> = response
            .answers
            .iter()
            .filter_map(|record| match &record.data {
                RData::A(address) => Some(address.0),
                _ => None,
            })
            .collect();
        return Ok(if addresses.is_empty() {
            DnsAnswer::Code(code(response.metadata.response_code))
        } else {
            DnsAnswer::Addresses(addresses)
        });
    }
}

fn code(code: ResponseCode) -> String {
    match code {
        ResponseCode::NoError => "NOERROR".to_owned(),
        ResponseCode::NXDomain => "NXDOMAIN".to_owned(),
        ResponseCode::Refused => "REFUSED".to_owned(),
        ResponseCode::ServFail => "SERVFAIL".to_owned(),
        other => format!("{other:?}").to_uppercase(),
    }
}
