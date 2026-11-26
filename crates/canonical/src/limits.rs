/// The bounds a canonical reader enforces, each checked before the work it guards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Total input length in bytes.
    pub max_bytes: usize,
    /// Containers nested inside one another; the top-level value is depth one.
    pub max_depth: u32,
    /// Values in the whole document, counting every container and every scalar.
    pub max_values: u32,
    /// Length of one string, in bytes as they appear in the input (escapes included).
    pub max_string_bytes: usize,
    /// Members of one object, or elements of one array.
    pub max_members: u32,
}

impl Limits {
    pub(crate) const fn admit_len(&self, len: usize) -> Result<(), crate::CanonicalError> {
        if len > self.max_bytes {
            return Err(crate::CanonicalError::TooLarge {
                len,
                max: self.max_bytes,
            });
        }
        Ok(())
    }

    /// A published definition: 64 KiB, and bounds wide enough for the deepest legal one.
    pub const DOCUMENT: Self = Self {
        max_bytes: 65_536,
        max_depth: 24,
        max_values: 8_192,
        max_string_bytes: 4_096,
        max_members: 256,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DOCUMENT
    }
}
