//! [`Spelled`] — a value read leniently, and whether it was written in its one spelling.

/// A value read from text that accepts more than one spelling of it.
///
/// An exhaustive match makes whoever accepted a variant spelling decide what to do with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Spelled<T> {
    /// The text was the value's one spelling.
    Canonical(T),
    /// The text named the value in another spelling: another case, or a look-alike symbol.
    Variant(T),
}

impl<T> Spelled<T> {
    /// The value, however it was spelled.
    #[must_use]
    pub fn value(self) -> T {
        match self {
            Self::Canonical(value) | Self::Variant(value) => value,
        }
    }

    /// Whether the text was the value's one spelling.
    #[must_use]
    pub const fn is_canonical(&self) -> bool {
        matches!(self, Self::Canonical(_))
    }

    /// The same spelling of another value.
    #[must_use]
    pub fn map<U>(self, convert: impl FnOnce(T) -> U) -> Spelled<U> {
        match self {
            Self::Canonical(value) => Spelled::Canonical(convert(value)),
            Self::Variant(value) => Spelled::Variant(convert(value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Spelled;

    #[test]
    fn a_variant_keeps_its_value_and_says_it_was_not_canonical() {
        let variant = Spelled::Variant(7_u8);
        assert!(!variant.is_canonical());
        assert_eq!(variant.map(u16::from), Spelled::Variant(7_u16));
        assert_eq!(variant.value(), 7);
        assert!(Spelled::Canonical(()).is_canonical());
    }
}
