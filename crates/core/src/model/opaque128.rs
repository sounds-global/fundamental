#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;

use super::Spelled;
use super::base32_layout::{Base32Layout, SymbolReading};

/// 128 bits with two text forms, each with exactly one spelling.
///
/// The text form is 26 lower-case RFC 4648 base32 characters whose two padding bits must be zero; it is published,
/// so it never changes. The Crockford form is a TypeID v0.3 suffix, the only form with a lenient reading.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Opaque128([u8; 16]);

impl Opaque128 {
    /// The length of the text form.
    pub const LEN: usize = 26;

    /// Wrap 128 bits already drawn — from the OS random source, or a digest's prefix.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The 128 bits.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// The one text form.
    #[must_use]
    pub fn to_text(&self) -> String {
        spell(&Base32Layout::RFC4648_LOWER, &self.0)
    }

    /// Read the one text form, refusing every other spelling.
    ///
    /// # Errors
    ///
    /// [`MalformedOpaque128`] for a wrong length, a character outside the lower-case
    /// alphabet, or non-zero padding bits.
    pub const fn parse(text: &str) -> Result<Self, MalformedOpaque128> {
        match Base32Layout::RFC4648_LOWER.unpack(text.as_bytes(), SymbolReading::Strict) {
            Ok((bytes, _)) => Ok(Self(bytes)),
            Err(_) => Err(MalformedOpaque128),
        }
    }

    /// The TypeID v0.3 suffix: 26 lower-case Crockford characters, the first `0`–`7`.
    #[must_use]
    pub fn to_crockford(&self) -> String {
        spell(&Base32Layout::CROCKFORD, &self.0)
    }

    /// Read a TypeID v0.3 suffix in its one spelling, as the TypeID test vectors require.
    ///
    /// # Errors
    ///
    /// [`MalformedOpaque128`] for a wrong length, a character outside the lower-case
    /// Crockford alphabet, or a first character above `7` (more than 128 bits).
    pub const fn parse_crockford(text: &str) -> Result<Self, MalformedOpaque128> {
        match Base32Layout::CROCKFORD.unpack(text.as_bytes(), SymbolReading::Strict) {
            Ok((bytes, _)) => Ok(Self(bytes)),
            Err(_) => Err(MalformedOpaque128),
        }
    }

    /// Read a Crockford suffix the way a person types one: any case, `i` and `l` as `1`, `o`
    /// as `0`.
    ///
    /// # Errors
    ///
    /// [`MalformedOpaque128`] for a wrong length, a character that is not a Crockford symbol or
    /// look-alike (`u`, a hyphen, a space), or a first character above `7`.
    pub const fn read_crockford(text: &str) -> Result<Spelled<Self>, MalformedOpaque128> {
        match Base32Layout::CROCKFORD.unpack(text.as_bytes(), SymbolReading::Lenient) {
            Ok((bytes, true)) => Ok(Spelled::Canonical(Self(bytes))),
            Ok((bytes, false)) => Ok(Spelled::Variant(Self(bytes))),
            Err(_) => Err(MalformedOpaque128),
        }
    }
}

/// The 26 symbols `layout` writes for `bytes`, which are ASCII by construction.
fn spell(layout: &Base32Layout, bytes: &[u8; 16]) -> String {
    layout
        .pack(bytes)
        .iter()
        .map(|&symbol| char::from(symbol))
        .collect()
}

impl fmt::Display for Opaque128 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_text())
    }
}

impl fmt::Debug for Opaque128 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Opaque128({})", self.to_text())
    }
}

/// A text that is not a spelling of an [`Opaque128`] the reader accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MalformedOpaque128;

impl fmt::Display for MalformedOpaque128 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("not 26 lower-case base32 characters with zero padding bits")
    }
}

#[cfg(test)]
mod tests {
    use super::{MalformedOpaque128, Opaque128};
    use crate::model::Spelled;

    #[test]
    fn rfc_4648_vectors_hold_lower_cased() {
        // RFC 4648 §10: BASE32("foobar") = "MZXW6YTBOI======". Its first nine characters take
        // bits from "foobar" alone; the tenth takes two from whatever follows.
        let bytes = *b"foobarfoobarfoob";
        let text = Opaque128::from_bytes(bytes).to_text();
        assert!(text.starts_with("mzxw6ytbo"), "{text}");
        assert_eq!(Opaque128::parse(&text), Ok(Opaque128::from_bytes(bytes)));
    }

    #[test]
    fn the_text_form_is_pinned() {
        let bytes: [u8; 16] = core::array::from_fn(|index| index as u8);
        assert_eq!(
            Opaque128::from_bytes(bytes).to_text(),
            "aaaqeayeaudaocajbifqydiob4"
        );
    }

    #[test]
    fn the_crockford_form_round_trips_and_differs_from_the_text_form() {
        let value = Opaque128::from_bytes([0xa5; 16]);
        let crockford = value.to_crockford();
        assert_ne!(crockford, value.to_text());
        assert_eq!(Opaque128::parse_crockford(&crockford), Ok(value));
        assert_eq!(
            Opaque128::read_crockford(&crockford),
            Ok(Spelled::Canonical(value))
        );
        assert_eq!(
            Opaque128::read_crockford(&crockford.to_uppercase()),
            Ok(Spelled::Variant(value))
        );
        assert_eq!(
            Opaque128::parse_crockford(&crockford.to_uppercase()),
            Err(MalformedOpaque128)
        );
    }

    #[test]
    fn every_value_round_trips() {
        for seed in 0_u8..=255 {
            let bytes = [seed.wrapping_mul(37); 16].map(|byte| byte ^ seed);
            let value = Opaque128::from_bytes(bytes);
            assert_eq!(Opaque128::parse(&value.to_text()), Ok(value));
        }
    }

    #[test]
    fn nonzero_padding_bits_are_a_second_spelling_and_refused() {
        let text = Opaque128::from_bytes([0; 16]).to_text();
        assert_eq!(text, "aaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(
            Opaque128::parse("aaaaaaaaaaaaaaaaaaaaaaaaab"),
            Err(MalformedOpaque128)
        );
    }

    #[test]
    fn upper_case_and_other_lengths_are_refused() {
        assert_eq!(
            Opaque128::parse("AAAAAAAAAAAAAAAAAAAAAAAAAA"),
            Err(MalformedOpaque128)
        );
        assert_eq!(
            Opaque128::parse("aaaaaaaaaaaaaaaaaaaaaaaaa"),
            Err(MalformedOpaque128)
        );
        assert_eq!(
            Opaque128::parse("aaaaaaaaaaaaaaaaaaaaaaaaa1"),
            Err(MalformedOpaque128)
        );
    }

    #[test]
    fn a_literal_is_checked_when_it_compiles() {
        const ZERO: Result<Opaque128, MalformedOpaque128> =
            Opaque128::parse("aaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(ZERO, Ok(Opaque128::from_bytes([0; 16])));
    }
}
