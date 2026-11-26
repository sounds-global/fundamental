use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::Signature as EcdsaSignature;

use crate::KeyError;

/// An ECDSA P-256 signature in the fixed-width `r ‖ s` form (64 bytes), with `s`
/// normalised to the lower half of the group order.
///
/// Normalised so one message and key have exactly one encoding.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct P256Signature([u8; 64]);

impl P256Signature {
    /// Parses 64 bytes of `r ‖ s`, normalising a high `s`.
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] if `r` or `s` is zero or not below the order.
    pub fn from_bytes(bytes: &[u8; 64]) -> Result<Self, KeyError> {
        let parsed = EcdsaSignature::from_slice(bytes)
            .map_err(|_| KeyError::Malformed("not a valid P-256 signature".to_owned()))?;
        let normalised = parsed.normalize_s().unwrap_or(parsed);
        let mut out = [0u8; 64];
        out.copy_from_slice(&normalised.to_bytes());
        Ok(Self(out))
    }

    /// Parses a slice that must be exactly 64 bytes of `r ‖ s`.
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] for any other length or an invalid signature.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, KeyError> {
        let fixed: &[u8; 64] = bytes
            .try_into()
            .map_err(|_| KeyError::Malformed("a P-256 signature is 64 bytes".to_owned()))?;
        Self::from_bytes(fixed)
    }

    /// The 64 bytes of `r ‖ s`.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 64] {
        self.0
    }
}

impl fmt::Debug for P256Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P256Signature({})", URL_SAFE_NO_PAD.encode(self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::P256VerifyingKey;
    use p256::ecdsa::SigningKey as EcdsaSigningKey;
    use p256::ecdsa::signature::Signer as _;
    use p256::elliptic_curve::scalar::IsHigh;

    #[test]
    fn a_high_s_signature_is_normalised_and_still_verifies() {
        let key = EcdsaSigningKey::from_slice(&[9u8; 32]).expect("valid scalar");
        let message = b"statement";
        let signed: EcdsaSignature = key.sign(message);
        let high = if bool::from(signed.s().is_high()) {
            signed
        } else {
            let (r, s) = signed.split_scalars();
            EcdsaSignature::from_scalars(r, -*s).expect("negated s is valid")
        };
        assert!(bool::from(high.s().is_high()));

        let mut raw = [0u8; 64];
        raw.copy_from_slice(&high.to_bytes());
        let parsed = P256Signature::from_bytes(&raw).expect("parses");
        assert_ne!(parsed.to_bytes(), raw, "the high s was rewritten");

        let public =
            P256VerifyingKey::from_sec1(key.verifying_key().to_encoded_point(false).as_bytes())
                .expect("valid key");
        public
            .verify(message, &parsed)
            .expect("the normalised form verifies");
    }

    #[test]
    fn a_zero_signature_is_refused() {
        assert!(matches!(
            P256Signature::from_bytes(&[0u8; 64]),
            Err(KeyError::Malformed(_))
        ));
    }

    #[test]
    fn a_wrong_length_is_refused() {
        assert!(P256Signature::from_slice(&[1u8; 63]).is_err());
    }
}
