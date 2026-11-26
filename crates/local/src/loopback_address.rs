use std::fmt;
use std::net::Ipv4Addr;

use crate::LoopbackPool;

/// The loopback address one instance owns: `127.0.<block>.2` to `127.0.<block>.254`, the block
/// its [`LoopbackPool`]'s; main checkouts claim one too, so two never both want `127.0.0.1:443`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LoopbackAddress {
    block: u8,
    host: u8,
}

impl LoopbackAddress {
    /// The first address an instance may claim.
    pub const FIRST: u8 = 2;
    /// The last address an instance may claim.
    pub const LAST: u8 = 254;

    /// The machine's address whose last octet is `octet`, or `None` outside the pool.
    #[must_use]
    pub const fn new(octet: u8) -> Option<Self> {
        Self::in_block(LoopbackPool::MACHINE.block(), octet)
    }

    /// The address `127.0.<block>.<octet>`, or `None` when `octet` is outside the pool.
    pub(crate) const fn in_block(block: u8, octet: u8) -> Option<Self> {
        if octet >= Self::FIRST && octet <= Self::LAST {
            Some(Self { block, host: octet })
        } else {
            None
        }
    }

    /// Parses an address of the machine's pool or the tests'.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let address: Ipv4Addr = text.parse().ok()?;
        match address.octets() {
            [127, 0, block, octet] if LoopbackPool::of_block(block).is_some() => {
                Self::in_block(block, octet)
            }
            _ => None,
        }
    }

    /// Every address in the machine's pool, lowest first.
    pub fn pool() -> impl Iterator<Item = Self> {
        LoopbackPool::MACHINE.addresses()
    }

    /// The third octet: which pool the address is of.
    #[must_use]
    pub const fn block(self) -> u8 {
        self.block
    }

    /// The address as an [`Ipv4Addr`].
    #[must_use]
    pub const fn ip(self) -> Ipv4Addr {
        Ipv4Addr::new(127, 0, self.block, self.host)
    }
}

impl fmt::Display for LoopbackAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "127.0.{}.{}", self.block, self.host)
    }
}

#[cfg(test)]
mod tests {
    use super::LoopbackAddress;
    use crate::LoopbackPool;

    #[test]
    fn the_pool_is_two_to_two_hundred_fifty_four() {
        assert_eq!(LoopbackAddress::pool().count(), 253);
        assert_eq!(
            LoopbackAddress::parse("127.0.0.2")
                .map(|a| a.to_string())
                .as_deref(),
            Some("127.0.0.2")
        );
        for text in ["127.0.0.1", "127.0.0.255", "127.0.1.2", "10.0.0.2", "x"] {
            assert_eq!(LoopbackAddress::parse(text), None, "{text}");
        }
    }

    #[test]
    fn the_tests_pool_shares_no_address_with_the_machines() {
        let machine: Vec<LoopbackAddress> = LoopbackPool::MACHINE.addresses().collect();
        assert_eq!(machine, LoopbackAddress::pool().collect::<Vec<_>>());
        for address in LoopbackPool::TESTS.addresses() {
            assert!(!machine.contains(&address), "{address}");
            assert!(!LoopbackPool::MACHINE.holds(address), "{address}");
            assert_eq!(
                LoopbackAddress::parse(&address.to_string()),
                Some(address),
                "a test registry reads back what it wrote"
            );
        }
        assert_eq!(
            LoopbackPool::TESTS
                .addresses()
                .next()
                .map(|a| a.to_string())
                .as_deref(),
            Some("127.0.77.2")
        );
    }
}
