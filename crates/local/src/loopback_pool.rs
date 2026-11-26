use crate::LoopbackAddress;

/// A block of loopback addresses one registry hands out: the machine's `127.0.0.x`, or the
/// tests' `127.0.77.x`, so test edges never touch the machine's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LoopbackPool {
    block: u8,
}

impl LoopbackPool {
    /// The machine's: every instance `fun local up` serves.
    pub const MACHINE: Self = Self { block: 0 };
    /// The tests': never the machine's, nor Debian's `127.0.1.1` for the host name.
    pub const TESTS: Self = Self { block: 77 };

    /// The pool whose third octet is `block`, of the two there are.
    #[must_use]
    pub const fn of_block(block: u8) -> Option<Self> {
        match block {
            0 => Some(Self::MACHINE),
            77 => Some(Self::TESTS),
            _ => None,
        }
    }

    /// The third octet of every address in the pool.
    #[must_use]
    pub const fn block(self) -> u8 {
        self.block
    }

    /// Whether `address` is one of the pool's.
    #[must_use]
    pub const fn holds(self, address: LoopbackAddress) -> bool {
        address.block() == self.block
    }

    /// Every address in the pool, lowest first.
    pub fn addresses(self) -> impl Iterator<Item = LoopbackAddress> {
        (LoopbackAddress::FIRST..=LoopbackAddress::LAST)
            .filter_map(move |octet| LoopbackAddress::in_block(self.block, octet))
    }
}
