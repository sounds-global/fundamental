//! A span of bytes within an object.

/// The bytes `first..=last` of an object — the shape an HTTP `Range` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    first: u64,
    last: u64,
}

impl ByteRange {
    /// The bytes `first..=last`, or `None` if `last` comes before `first`.
    #[must_use]
    pub fn new(first: u64, last: u64) -> Option<Self> {
        (first <= last).then_some(Self { first, last })
    }

    /// The range within an object of `len` bytes: its end clamped to the object's, or `None` if
    /// it starts past the object — what HTTP calls unsatisfiable.
    #[must_use]
    pub fn within(self, len: u64) -> Option<Self> {
        (self.first < len).then(|| Self {
            first: self.first,
            last: self.last.min(len - 1),
        })
    }

    /// The first byte.
    #[must_use]
    pub const fn first(self) -> u64 {
        self.first
    }

    /// The last byte.
    #[must_use]
    pub const fn last(self) -> u64 {
        self.last
    }

    /// How many bytes it spans.
    #[must_use]
    pub const fn len(self) -> u64 {
        self.last - self.first + 1
    }

    /// Never true: a range spans at least one byte.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::ByteRange;

    #[test]
    fn a_range_is_clamped_to_its_object_or_unsatisfiable() {
        let range = ByteRange::new(10, 99).expect("ordered");
        assert_eq!(range.within(50), ByteRange::new(10, 49));
        assert_eq!(range.within(10), None);
        assert_eq!(range.len(), 90);
        assert_eq!(ByteRange::new(5, 4), None);
    }
}
