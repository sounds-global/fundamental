use std::fmt;

/// A JWS in compact serialization: `header.payload.signature`, each base64url.
#[derive(Clone, PartialEq, Eq)]
pub struct CompactJws(String);

impl CompactJws {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    /// The compact serialization.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Takes the compact serialization.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for CompactJws {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for CompactJws {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CompactJws({} bytes)", self.0.len())
    }
}
