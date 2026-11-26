/// `count ‖ (len ‖ part)*` in u64 little-endian, so no two part sequences encode alike.
pub(crate) fn length_prefixed<'a>(parts: impl ExactSizeIterator<Item = &'a [u8]>) -> Vec<u8> {
    let mut framed = Vec::with_capacity(8 * (parts.len() + 1));
    framed.extend_from_slice(&length_prefix(parts.len()));
    for part in parts {
        framed.extend_from_slice(&length_prefix(part.len()));
        framed.extend_from_slice(part);
    }
    framed
}

pub(crate) fn length_prefix(len: usize) -> [u8; 8] {
    u64::try_from(len).unwrap_or(u64::MAX).to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::length_prefixed;

    #[test]
    fn a_boundary_moved_between_parts_changes_the_framing() {
        let split_early: [&[u8]; 2] = [b"a", b"bc"];
        let split_late: [&[u8]; 2] = [b"ab", b"c"];
        assert_ne!(
            length_prefixed(split_early.into_iter()),
            length_prefixed(split_late.into_iter())
        );
    }

    #[test]
    fn the_framing_is_the_count_then_each_length_and_part() {
        let parts: [&[u8]; 2] = [b"x", b""];
        let mut expected = Vec::new();
        expected.extend_from_slice(&2_u64.to_le_bytes());
        expected.extend_from_slice(&1_u64.to_le_bytes());
        expected.extend_from_slice(b"x");
        expected.extend_from_slice(&0_u64.to_le_bytes());
        assert_eq!(length_prefixed(parts.into_iter()), expected);
    }
}
