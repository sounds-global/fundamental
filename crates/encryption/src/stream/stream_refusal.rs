use std::fmt;

/// Why a [`SegmentOpener`](crate::SegmentOpener) refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamRefusal {
    /// The header is not this format: wrong magic, version or segment size.
    Header,
    /// A segment is not the length its position requires.
    Length,
    /// Altered, out of place, or sealed under another key or binding.
    Unauthenticated,
}

impl StreamRefusal {
    /// The refusal's name, for a span field or a metric label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Length => "length",
            Self::Unauthenticated => "unauthenticated",
        }
    }
}

impl fmt::Display for StreamRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::StreamRefusal;

    #[test]
    fn every_refusal_has_its_own_label() {
        let labels = [
            StreamRefusal::Header,
            StreamRefusal::Length,
            StreamRefusal::Unauthenticated,
        ]
        .map(StreamRefusal::as_str);
        for (index, label) in labels.iter().enumerate() {
            assert!(
                !labels[index + 1..].contains(label),
                "{label} is used twice"
            );
        }
    }
}
