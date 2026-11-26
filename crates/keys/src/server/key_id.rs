use std::fmt;

/// A stable, logical name for a [`Signer`](super::Signer)'s key.
///
/// Not key material; protocol-specific ids are derived from it by the protocol layer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyId(String);

impl KeyId {
    /// Wraps a logical key identifier.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for KeyId {
    fn from(id: &str) -> Self {
        Self::new(id)
    }
}

impl From<String> for KeyId {
    fn from(id: String) -> Self {
        Self::new(id)
    }
}
