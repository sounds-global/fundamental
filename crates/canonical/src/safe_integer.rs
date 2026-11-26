use crate::CanonicalError;
use crate::canonical_json::{CanonicalJson, write_decimal};

/// An integer every JSON implementation reads exactly: within ±(2⁵³−1) (I-JSON, RFC 7493 §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SafeInteger(i64);

impl SafeInteger {
    /// The largest value: 2⁵³ − 1.
    pub const MAX: Self = Self((1 << 53) - 1);
    /// The smallest value: −(2⁵³ − 1).
    pub const MIN: Self = Self(-((1 << 53) - 1));

    /// The integer.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for SafeInteger {
    type Error = CanonicalError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if (Self::MIN.0..=Self::MAX.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(CanonicalError::Unrepresentable {
                detail: "an integer outside ±(2⁵³−1)".to_owned(),
            })
        }
    }
}

impl TryFrom<u64> for SafeInteger {
    type Error = CanonicalError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        i64::try_from(value)
            .map_err(|_| CanonicalError::Unrepresentable {
                detail: "an integer outside ±(2⁵³−1)".to_owned(),
            })
            .and_then(Self::try_from)
    }
}

impl CanonicalJson for SafeInteger {
    fn write_canonical_json(&self, out: &mut Vec<u8>) {
        write_decimal(out, self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::SafeInteger;
    use crate::{CanonicalError, CanonicalJson};

    #[test]
    fn the_bounds_are_exactly_two_to_the_fifty_three_less_one() {
        assert_eq!(
            SafeInteger::try_from(9_007_199_254_740_991_i64).ok(),
            Some(SafeInteger::MAX)
        );
        assert_eq!(
            SafeInteger::try_from(-9_007_199_254_740_991_i64).ok(),
            Some(SafeInteger::MIN)
        );
        for outside in [
            9_007_199_254_740_992_i64,
            -9_007_199_254_740_992,
            i64::MAX,
            i64::MIN,
        ] {
            assert!(matches!(
                SafeInteger::try_from(outside),
                Err(CanonicalError::Unrepresentable { .. })
            ));
        }
        assert!(SafeInteger::try_from(u64::MAX).is_err());
    }

    #[test]
    fn it_writes_as_the_writer_writes() {
        assert_eq!(SafeInteger::MIN.to_canonical_json(), b"-9007199254740991");
        assert_eq!(
            SafeInteger::MAX.to_canonical_json(),
            crate::to_canonical_vec(&9_007_199_254_740_991_i64).expect("in range")
        );
    }
}
