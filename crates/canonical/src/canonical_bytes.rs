use crate::{CanonicalError, CanonicalJson, to_canonical_vec};
use serde::Serialize;

/// The canonical encoding of a value whose shape is not known at compile time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalBytes(Vec<u8>);

impl CanonicalBytes {
    /// The canonical encoding of `value`.
    ///
    /// # Errors
    ///
    /// [`CanonicalError::Unrepresentable`] when `value` has no canonical encoding.
    pub fn of<T: Serialize + ?Sized>(value: &T) -> Result<Self, CanonicalError> {
        to_canonical_vec(value).map(Self)
    }

    /// The encoded bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl CanonicalJson for CanonicalBytes {
    fn write_canonical_json(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::CanonicalBytes;
    use crate::{CanonicalError, CanonicalJson};
    use std::collections::BTreeMap;

    #[test]
    fn it_carries_a_values_canonical_encoding_verbatim() {
        let mut value = BTreeMap::new();
        value.insert("b", 2_u8);
        value.insert("a", 1_u8);
        let bytes = CanonicalBytes::of(&value).expect("integers are representable");
        assert_eq!(bytes.as_bytes(), br#"{"a":1,"b":2}"#);
        assert_eq!(bytes.to_canonical_json(), br#"{"a":1,"b":2}"#);
    }

    #[test]
    fn a_float_has_no_canonical_bytes() {
        assert!(matches!(
            CanonicalBytes::of(&1.5_f64),
            Err(CanonicalError::Unrepresentable { .. })
        ));
    }
}
