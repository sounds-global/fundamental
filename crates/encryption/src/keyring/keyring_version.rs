use std::fmt;
use std::num::NonZeroU16;

/// The version of one key in a [`Keyring`](crate::Keyring).
///
/// Non-zero, so a zeroed header or defaulted column never names a real key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyringVersion(NonZeroU16);

impl KeyringVersion {
    /// The version numbered `number`, or `None` for zero.
    #[must_use]
    pub fn new(number: u16) -> Option<Self> {
        NonZeroU16::new(number).map(Self)
    }

    /// The version's number.
    #[must_use]
    pub fn get(self) -> u16 {
        self.0.get()
    }
}

impl fmt::Display for KeyringVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::KeyringVersion;

    #[test]
    fn zero_is_not_a_version() {
        assert_eq!(KeyringVersion::new(0), None);
    }

    #[test]
    fn a_version_round_trips_its_number() {
        assert_eq!(KeyringVersion::new(7).map(KeyringVersion::get), Some(7));
    }
}
