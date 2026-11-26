use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::KeyError;

/// The shortest signature accepted, in bytes: a 2048-bit modulus.
const MIN_BYTES: usize = 256;
/// The longest signature accepted, in bytes: a 4096-bit modulus.
const MAX_BYTES: usize = 512;

/// An RSASSA-PKCS1-v1_5 signature of 256 to 512 bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct RsaSignature(Vec<u8>);

impl RsaSignature {
    /// The signature `bytes`.
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] for fewer than 256 or more than 512 bytes.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, KeyError> {
        if (MIN_BYTES..=MAX_BYTES).contains(&bytes.len()) {
            Ok(Self(bytes.to_vec()))
        } else {
            Err(KeyError::Malformed(format!(
                "an RSA signature is {MIN_BYTES} to {MAX_BYTES} bytes, not {}",
                bytes.len()
            )))
        }
    }

    /// The signature's bytes, as JOSE carries them.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for RsaSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RsaSignature({})", URL_SAFE_NO_PAD.encode(&self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_outside_2048_to_4096_bits_is_refused() {
        for length in [0, 255, 513] {
            assert!(matches!(
                RsaSignature::from_slice(&vec![1; length]),
                Err(KeyError::Malformed(_))
            ));
        }
        for length in [256, 384, 512] {
            assert_eq!(
                RsaSignature::from_slice(&vec![1; length])
                    .expect("accepted")
                    .as_bytes()
                    .len(),
                length
            );
        }
    }
}
