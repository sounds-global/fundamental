//! [`Base32Layout`] — the one bit-packing routine behind every 26-character spelling of 128 bits.

/// How 128 bits become 26 base32 symbols: which alphabet, and where the two zero padding bits sit.
///
/// RFC 4648 puts them at the end, TypeID at the front; both share one loop so a fix to either fixes both.
pub(crate) struct Base32Layout {
    alphabet: &'static [u8; 32],
    leading_pad_bits: u32,
    look_alikes: bool,
}

/// How strictly symbols are read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolReading {
    /// Exactly the lower-case alphabet: the one spelling.
    Strict,
    /// Any case, and — for a layout that allows it — Crockford's look-alikes.
    Lenient,
}

/// A text that is not 26 symbols of the layout with zero padding bits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Unreadable;

impl Base32Layout {
    /// The length of every spelling.
    pub(crate) const LEN: usize = 26;

    /// RFC 4648 §6, lower-cased, padding bits trailing: `Opaque128`'s published text.
    pub(crate) const RFC4648_LOWER: Self = Self {
        alphabet: b"abcdefghijklmnopqrstuvwxyz234567",
        leading_pad_bits: 0,
        look_alikes: false,
    };

    /// Crockford's alphabet with the padding bits leading: the TypeID v0.3 suffix; `u` and separators are refused even leniently.
    pub(crate) const CROCKFORD: Self = Self {
        alphabet: b"0123456789abcdefghjkmnpqrstvwxyz",
        leading_pad_bits: 2,
        look_alikes: true,
    };

    /// The one spelling of `bytes`.
    pub(crate) const fn pack(&self, bytes: &[u8; 16]) -> [u8; 26] {
        let mut out = [0_u8; 26];
        let mut buffer: u32 = 0;
        let mut bits = self.leading_pad_bits;
        let mut written = 0;
        let mut index = 0;
        while index < 16 {
            buffer = (buffer << 8) | bytes[index] as u32;
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                out[written] = self.alphabet[((buffer >> bits) & 31) as usize];
                written += 1;
            }
            buffer &= (1 << bits) - 1;
            index += 1;
        }
        if bits > 0 {
            out[written] = self.alphabet[((buffer << (5 - bits)) & 31) as usize];
        }
        out
    }

    /// The bytes `text` spells, and whether it is the one spelling.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(crate) const fn unpack(
        &self,
        text: &[u8],
        reading: SymbolReading,
    ) -> Result<([u8; 16], bool), Unreadable> {
        if text.len() != Self::LEN {
            return Err(Unreadable);
        }
        let mut out = [0_u8; 16];
        let mut canonical = true;
        let mut buffer: u32 = 0;
        let mut bits: u32 = 0;
        let mut written = 0;
        let mut index = 0;
        while index < Self::LEN {
            let (value, exact) = match self.value_of(text[index], reading) {
                Some(found) => found,
                None => return Err(Unreadable),
            };
            canonical = canonical && exact;
            let payload_bits = if index == 0 {
                5 - self.leading_pad_bits
            } else {
                5
            };
            if (value as u32) >> payload_bits != 0 {
                return Err(Unreadable);
            }
            buffer = (buffer << payload_bits) | value as u32;
            bits += payload_bits;
            if bits >= 8 {
                bits -= 8;
                if written < 16 {
                    out[written] = (buffer >> bits) as u8;
                    written += 1;
                }
            }
            buffer &= (1 << bits) - 1;
            index += 1;
        }
        if buffer != 0 {
            return Err(Unreadable);
        }
        Ok((out, canonical))
    }

    /// The value of one symbol, and whether it was spelled exactly.
    const fn value_of(&self, symbol: u8, reading: SymbolReading) -> Option<(u8, bool)> {
        if let Some(value) = self.position(symbol) {
            return Some((value, true));
        }
        if !matches!(reading, SymbolReading::Lenient) {
            return None;
        }
        let folded = match symbol.to_ascii_lowercase() {
            b'i' | b'l' if self.look_alikes => b'1',
            b'o' if self.look_alikes => b'0',
            other => other,
        };
        match self.position(folded) {
            Some(value) => Some((value, false)),
            None => None,
        }
    }

    const fn position(&self, symbol: u8) -> Option<u8> {
        let mut value = 0;
        while value < 32 {
            if self.alphabet[value] == symbol {
                return Some(value as u8);
            }
            value += 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{Base32Layout, SymbolReading, Unreadable};

    fn text(layout: &Base32Layout, bytes: [u8; 16]) -> String {
        String::from_utf8(layout.pack(&bytes).to_vec()).unwrap_or_default()
    }

    #[test]
    fn crockford_puts_the_padding_first() {
        assert_eq!(
            text(&Base32Layout::CROCKFORD, [0xff; 16]),
            "7zzzzzzzzzzzzzzzzzzzzzzzzz"
        );
        assert_eq!(
            text(&Base32Layout::CROCKFORD, [0; 16]),
            "00000000000000000000000000"
        );
    }

    #[test]
    fn rfc4648_puts_the_padding_last() {
        assert_eq!(
            text(&Base32Layout::RFC4648_LOWER, [0xff; 16]),
            "77777777777777777777777774"
        );
    }

    #[test]
    fn both_layouts_round_trip_every_byte_pattern() {
        for layout in [Base32Layout::RFC4648_LOWER, Base32Layout::CROCKFORD] {
            for seed in 0_u8..=255 {
                let bytes: [u8; 16] =
                    core::array::from_fn(|index| seed.wrapping_mul(index as u8 + 31) ^ 0x5a);
                let spelled = layout.pack(&bytes);
                assert_eq!(
                    layout.unpack(&spelled, SymbolReading::Strict),
                    Ok((bytes, true))
                );
            }
        }
    }

    #[test]
    fn a_leading_symbol_above_seven_overflows_and_is_refused() {
        assert_eq!(
            Base32Layout::CROCKFORD.unpack(b"8zzzzzzzzzzzzzzzzzzzzzzzzz", SymbolReading::Lenient),
            Err(Unreadable)
        );
    }

    #[test]
    fn lenient_crockford_reads_look_alikes_as_a_variant() {
        let exact =
            Base32Layout::CROCKFORD.unpack(b"01111000000000000000000000", SymbolReading::Strict);
        let variant =
            Base32Layout::CROCKFORD.unpack(b"0ILi1O000000000000000000O0", SymbolReading::Lenient);
        assert_eq!(exact.map(|(bytes, _)| (bytes, false)), variant);
    }

    #[test]
    fn strict_reading_refuses_what_lenient_reading_folds() {
        assert_eq!(
            Base32Layout::CROCKFORD.unpack(b"0000000000000000000000000O", SymbolReading::Strict),
            Err(Unreadable)
        );
    }

    #[test]
    fn lenient_reading_still_refuses_u_and_separators() {
        for refused in [
            b"0000000000000000000000000u".as_slice(),
            b"000000000000-0000000000000".as_slice(),
        ] {
            assert_eq!(
                Base32Layout::CROCKFORD.unpack(refused, SymbolReading::Lenient),
                Err(Unreadable)
            );
        }
    }

    #[test]
    fn rfc4648_has_no_look_alikes_even_read_leniently() {
        assert_eq!(
            Base32Layout::RFC4648_LOWER
                .unpack(b"aaaaaaaaaaaaaaaaaaaaaaaaa1", SymbolReading::Lenient),
            Err(Unreadable)
        );
    }
}
