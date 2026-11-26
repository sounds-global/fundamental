use std::fmt;
use std::net::SocketAddr;

/// Who a request's connection came from, as the server itself observed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Peer {
    /// A connection from this network address.
    Network(SocketAddr),
    /// A connection on a socket the service manager passed: a unix socket with no address.
    Activated,
}

impl Peer {
    /// The network address, when the connection came from one.
    #[must_use]
    pub const fn address(self) -> Option<SocketAddr> {
        match self {
            Self::Network(address) => Some(address),
            Self::Activated => None,
        }
    }
}

impl From<SocketAddr> for Peer {
    fn from(address: SocketAddr) -> Self {
        Self::Network(address)
    }
}

/// A network peer shows as its address; a passed socket's as `activated`.
impl fmt::Display for Peer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(address) => address.fmt(f),
            Self::Activated => f.write_str("activated"),
        }
    }
}
