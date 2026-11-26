//! [`Listen`] — where a server listens.

use std::fmt;
use std::net::SocketAddr;

use super::SocketName;

/// Where a server listens: an address it binds itself, or the socket of that name the service
/// manager bound and passed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listen {
    /// An address the server binds; port `0` lets the system choose one.
    Address(SocketAddr),
    /// The passed socket of this name.
    Activated(SocketName),
}

impl Listen {
    /// The address, when the server binds one.
    #[must_use]
    pub const fn address(&self) -> Option<SocketAddr> {
        match self {
            Self::Address(address) => Some(*address),
            Self::Activated(_) => None,
        }
    }
}

impl From<SocketAddr> for Listen {
    fn from(address: SocketAddr) -> Self {
        Self::Address(address)
    }
}

impl fmt::Display for Listen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Address(address) => address.fmt(f),
            Self::Activated(name) => write!(f, "activated:{name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_shows_as_the_address_does() {
        let address: SocketAddr = "127.0.0.1:11001".parse().expect("an address");
        let listen = Listen::from(address);
        assert_eq!(listen.to_string(), address.to_string());
        assert_eq!(listen.address(), Some(address));
    }

    #[test]
    fn a_passed_socket_has_no_address() {
        let listen = Listen::Activated(SocketName::parse("web").expect("a name"));
        assert_eq!(listen.to_string(), "activated:web");
        assert_eq!(listen.address(), None);
    }
}
