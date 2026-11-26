/// The fixed geometry of a sealed stream: a header, then segments of
/// [`SEGMENT_LEN`](Self::SEGMENT_LEN) plaintext bytes each followed by a tag, the last one
/// shorter (or empty) and sealed as last.
pub struct StreamLayout;

impl StreamLayout {
    /// Plaintext bytes in every segment but the last.
    pub const SEGMENT_LEN: usize = 1 << Self::SEGMENT_LOG2;
    /// `log2` of [`SEGMENT_LEN`](Self::SEGMENT_LEN), as the header records it.
    pub const SEGMENT_LOG2: u8 = 16;
    /// Bytes of the header that opens every stream.
    pub const HEADER_LEN: usize = 24;
    /// Bytes each segment's authentication tag adds.
    pub const TAG_LEN: usize = 16;

    /// How many segments `plaintext_len` bytes seal as: at least one, so an empty plaintext still
    /// ends in a last segment.
    #[must_use]
    pub fn segment_count(plaintext_len: u64) -> u64 {
        plaintext_len.div_ceil(Self::segment_len()).max(1)
    }

    /// The sealed stream's total length for `plaintext_len` bytes, or `None` if it would not
    /// fit in a `u64` or need more segments than the STREAM counter numbers.
    #[must_use]
    pub fn ciphertext_len(plaintext_len: u64) -> Option<u64> {
        let segments = Self::segment_count(plaintext_len);
        if segments > u64::from(u32::MAX) + 1 {
            return None;
        }
        segments
            .checked_mul(Self::tag_len())?
            .checked_add(plaintext_len)?
            .checked_add(Self::header_len())
    }

    /// The plaintext length a sealed stream of `ciphertext_len` bytes holds, or `None` if no
    /// plaintext seals to exactly that length.
    #[must_use]
    pub fn plaintext_len(ciphertext_len: u64) -> Option<u64> {
        let body = ciphertext_len.checked_sub(Self::header_len())?;
        let sealed_segment = Self::segment_len() + Self::tag_len();
        let full = body / sealed_segment;
        let rest = body % sealed_segment;
        let plaintext = if rest == 0 {
            full.checked_mul(Self::segment_len())?
        } else {
            let tail = rest.checked_sub(Self::tag_len())?;
            full.checked_mul(Self::segment_len())?.checked_add(tail)?
        };
        (Self::ciphertext_len(plaintext) == Some(ciphertext_len)).then_some(plaintext)
    }

    /// The byte range `(offset, len)` that segment `index` occupies in a stream sealing
    /// `plaintext_len` bytes, or `None` if the stream has no such segment.
    #[must_use]
    pub fn segment_range(index: u64, plaintext_len: u64) -> Option<(u64, u64)> {
        let segments = Self::segment_count(plaintext_len);
        if index >= segments {
            return None;
        }
        let sealed_segment = Self::segment_len() + Self::tag_len();
        let offset = index
            .checked_mul(sealed_segment)?
            .checked_add(Self::header_len())?;
        let plain = if index + 1 == segments {
            plaintext_len - index * Self::segment_len()
        } else {
            Self::segment_len()
        };
        Some((offset, plain + Self::tag_len()))
    }

    /// The segment holding plaintext byte `offset`.
    #[must_use]
    pub fn segment_of(offset: u64) -> u64 {
        offset / Self::segment_len()
    }

    fn segment_len() -> u64 {
        1 << Self::SEGMENT_LOG2
    }

    fn tag_len() -> u64 {
        Self::TAG_LEN as u64
    }

    fn header_len() -> u64 {
        Self::HEADER_LEN as u64
    }
}

#[cfg(test)]
mod tests {
    use super::StreamLayout;

    const SEGMENT: u64 = StreamLayout::SEGMENT_LEN as u64;

    #[test]
    fn an_empty_plaintext_is_one_tagged_segment() {
        assert_eq!(StreamLayout::segment_count(0), 1);
        assert_eq!(StreamLayout::ciphertext_len(0), Some(24 + 16));
    }

    #[test]
    fn lengths_round_trip_at_every_boundary() {
        for plaintext in [
            0,
            1,
            SEGMENT - 1,
            SEGMENT,
            SEGMENT + 1,
            3 * SEGMENT,
            25_000_000,
        ] {
            let sealed = StreamLayout::ciphertext_len(plaintext).expect("in range");
            assert_eq!(
                StreamLayout::plaintext_len(sealed),
                Some(plaintext),
                "{plaintext}"
            );
        }
    }

    #[test]
    fn a_length_no_plaintext_seals_to_is_refused() {
        let one_segment_and_a_partial_tag = 24 + SEGMENT + 16 + 5;
        for wrong in [0, 23, 24, 39, one_segment_and_a_partial_tag] {
            assert_eq!(StreamLayout::plaintext_len(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn segment_ranges_tile_the_stream() {
        let plaintext = 2 * SEGMENT + 5;
        let ranges: Vec<_> = (0..StreamLayout::segment_count(plaintext))
            .map(|index| StreamLayout::segment_range(index, plaintext).expect("a segment"))
            .collect();
        assert_eq!(ranges[0], (24, SEGMENT + 16));
        assert_eq!(ranges[1], (24 + SEGMENT + 16, SEGMENT + 16));
        assert_eq!(ranges[2], (24 + 2 * (SEGMENT + 16), 5 + 16));
        let end = ranges[2].0 + ranges[2].1;
        assert_eq!(Some(end), StreamLayout::ciphertext_len(plaintext));
        assert_eq!(StreamLayout::segment_range(3, plaintext), None);
    }

    #[test]
    fn more_segments_than_the_counter_numbers_has_no_length() {
        let too_many = (u64::from(u32::MAX) + 2) * SEGMENT;
        assert_eq!(StreamLayout::ciphertext_len(too_many), None);
    }
}
