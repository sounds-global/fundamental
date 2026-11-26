use std::net::Ipv4Addr;
use std::path::Path;

use crate::{LocalError, LoopbackAddress};

/// The two kinds of machine local origins run on, and what differs between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Linux with systemd-resolved: the resolver answers on the routing link's address.
    Linux,
    /// macOS: `/etc/resolver/test` sends `.test` to the resolver published on `127.0.0.1`.
    MacOs,
}

impl Platform {
    /// The address of the link `.test` is routed over on Linux, in TEST-NET-1 (RFC 5737).
    pub const ROUTING_ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 53);

    /// The name of that link.
    pub const ROUTING_LINK: &'static str = "fundamental-dns";

    /// The platform this process runs on.
    ///
    /// # Errors
    ///
    /// [`LocalError::UnsupportedPlatform`] (`LOCAL_013`) on Linux without systemd-resolved, and on
    /// anything that is neither Linux nor macOS.
    pub fn current() -> Result<Self, LocalError> {
        if cfg!(target_os = "macos") {
            return Ok(Self::MacOs);
        }
        if cfg!(target_os = "linux") {
            return if Path::new("/run/systemd/resolve").is_dir() {
                Ok(Self::Linux)
            } else {
                Err(LocalError::UnsupportedPlatform {
                    detail: "systemd-resolved is not running (/run/systemd/resolve is absent)"
                        .to_owned(),
                })
            };
        }
        Err(LocalError::UnsupportedPlatform {
            detail: std::env::consts::OS.to_owned(),
        })
    }

    /// Where the resolver listens: the routing link's address on Linux, loopback on macOS.
    #[must_use]
    pub const fn resolver_address(self) -> Ipv4Addr {
        match self {
            Self::Linux => Self::ROUTING_ADDRESS,
            Self::MacOs => Ipv4Addr::LOCALHOST,
        }
    }

    /// What the edge binds inside its container for `address`: the address itself on Linux (host
    /// network), every interface on macOS, where Docker's publish of `address:443` is the boundary.
    #[must_use]
    pub const fn edge_bind(self, address: LoopbackAddress) -> Ipv4Addr {
        match self {
            Self::Linux => address.ip(),
            Self::MacOs => Ipv4Addr::UNSPECIFIED,
        }
    }

    /// Where an app behind the instance at `address` listens: that address on Linux, loopback on
    /// macOS (the only one `host.docker.internal` reaches).
    #[must_use]
    pub const fn app_address(self, address: LoopbackAddress) -> Ipv4Addr {
        match self {
            Self::Linux => address.ip(),
            Self::MacOs => Ipv4Addr::LOCALHOST,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::Platform;
    use crate::LoopbackAddress;

    #[test]
    fn macos_binds_the_container_and_listens_where_the_edge_reaches() {
        let address = LoopbackAddress::new(9).expect("an address");
        assert_eq!(Platform::Linux.edge_bind(address), address.ip());
        assert_eq!(Platform::Linux.app_address(address), address.ip());
        assert_eq!(Platform::MacOs.edge_bind(address), Ipv4Addr::UNSPECIFIED);
        assert_eq!(Platform::MacOs.app_address(address), Ipv4Addr::LOCALHOST);
    }
}
