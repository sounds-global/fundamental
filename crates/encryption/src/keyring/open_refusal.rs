use std::fmt;

/// Why [`Keyring::open`](crate::Keyring::open) refused a token.
///
/// Never in the refusal's message or code; for a span field or metric label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenRefusal {
    /// Too short, or not this keyring's format.
    Malformed,
    /// Longer than the purpose's `MAX_PLAINTEXT_LEN` allows; refused before any work.
    TooLong,
    /// A key version this keyring does not hold — never issued, or retired.
    UnknownVersion,
    /// Altered, or sealed under another context or key.
    Unauthenticated,
}

impl OpenRefusal {
    /// The refusal's name, for a span field or a metric label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::TooLong => "too_long",
            Self::UnknownVersion => "unknown_version",
            Self::Unauthenticated => "unauthenticated",
        }
    }
}

impl fmt::Display for OpenRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::OpenRefusal;

    #[test]
    fn every_refusal_has_its_own_label() {
        let labels = [
            OpenRefusal::Malformed,
            OpenRefusal::TooLong,
            OpenRefusal::UnknownVersion,
            OpenRefusal::Unauthenticated,
        ]
        .map(OpenRefusal::as_str);
        for (index, label) in labels.iter().enumerate() {
            assert!(
                !labels[index + 1..].contains(label),
                "{label} is used twice"
            );
        }
    }
}
