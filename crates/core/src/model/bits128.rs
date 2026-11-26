//! [`Bits128`] — an identifier's value as exactly 128 bits.

/// A value that is exactly 128 bits, losslessly, in both directions: what a [`Handle`](super::Handle) needs of an id.
pub trait Bits128: Sized {
    /// The 128 bits, most significant first.
    fn to_bits(&self) -> [u8; 16];

    /// The value those bits are.
    fn from_bits(bits: [u8; 16]) -> Self;
}

impl Bits128 for uuid::Uuid {
    fn to_bits(&self) -> [u8; 16] {
        *self.as_bytes()
    }

    fn from_bits(bits: [u8; 16]) -> Self {
        Self::from_bytes(bits)
    }
}

impl Bits128 for u128 {
    fn to_bits(&self) -> [u8; 16] {
        self.to_be_bytes()
    }

    fn from_bits(bits: [u8; 16]) -> Self {
        Self::from_be_bytes(bits)
    }
}

impl Bits128 for [u8; 16] {
    fn to_bits(&self) -> [u8; 16] {
        *self
    }

    fn from_bits(bits: [u8; 16]) -> Self {
        bits
    }
}

#[cfg(test)]
mod tests {
    use super::Bits128;

    #[test]
    fn a_u128_is_big_endian_so_it_sorts_as_its_bits() {
        assert_eq!(1_u128.to_bits()[15], 1);
        assert_eq!(u128::from_bits(1_u128.to_bits()), 1);
    }

    #[test]
    fn a_uuid_keeps_its_bytes() {
        let uuid = uuid::Uuid::from_u128(0x0188_e5f5_f34a_7b3d_9f2a_6b1c_0d0e_0f10);
        assert_eq!(uuid::Uuid::from_bits(uuid.to_bits()), uuid);
    }
}
