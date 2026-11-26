//! [`PackageBounds`] — the most a package may hold.

/// The most a package may hold, checked by the side that writes one and again by the side that
/// unpacks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageBounds {
    /// Entries, `package.json` included.
    pub entries: usize,
    /// Bytes of every file unpacked, `package.json` included.
    pub unpacked: u64,
    /// Bytes of the tar.zst itself.
    pub packed: u64,
}

impl PackageBounds {
    /// A preview app's: 10 000 entries, 1 GiB unpacked, 256 MiB packed.
    pub const PREVIEW: Self = Self {
        entries: 10_000,
        unpacked: 1 << 30,
        packed: 256 << 20,
    };

    /// A production app's: the same as a preview's.
    pub const PRODUCTION: Self = Self::PREVIEW;
}
