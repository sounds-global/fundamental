use crate::CanonicalError;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use core::fmt;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

const PREFIX: &str = "sha256-";

/// The SHA-256 of some bytes, with its two standard spellings and no others.
#[derive(Clone, Copy)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    /// The digest of `bytes`.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// A digest already known, for a constant.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw 32 bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The W3C Subresource Integrity form: `sha256-` and 44 characters of padded base64.
    #[must_use]
    pub fn to_sri(&self) -> String {
        format!("{PREFIX}{}", STANDARD.encode(self.0))
    }

    /// The pin form used inside identifiers: `sha256-` and 43 characters of unpadded base64url.
    #[must_use]
    pub fn to_pin(&self) -> String {
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(self.0))
    }

    /// The bare form JOSE, SD-JWT and OpenID4VP carry: 43 characters of unpadded base64url.
    #[must_use]
    pub fn to_base64url(&self) -> String {
        crate::base64url32::encode(&self.0)
    }

    /// Parse the bare form, refusing every spelling but the canonical one.
    ///
    /// # Errors
    ///
    /// [`CanonicalError::MalformedDigest`] for any other string.
    pub fn parse_base64url(text: &str) -> Result<Self, CanonicalError> {
        crate::base64url32::decode(text)
            .map(Self)
            .ok_or(CanonicalError::MalformedDigest)
    }

    /// Parse the SRI form, refusing every spelling but the canonical one.
    ///
    /// # Errors
    ///
    /// [`CanonicalError::MalformedDigest`] for any other string.
    pub fn parse_sri(text: &str) -> Result<Self, CanonicalError> {
        Self::parse_with(text, 44, |body| STANDARD.decode(body).ok(), Self::to_sri)
    }

    /// Parse the pin form, refusing every spelling but the canonical one.
    ///
    /// # Errors
    ///
    /// [`CanonicalError::MalformedDigest`] for any other string.
    pub fn parse_pin(text: &str) -> Result<Self, CanonicalError> {
        Self::parse_with(
            text,
            43,
            |body| URL_SAFE_NO_PAD.decode(body).ok(),
            Self::to_pin,
        )
    }

    /// Refuse `bytes` unless they are the bytes this digest names, compared in constant time.
    ///
    /// # Errors
    ///
    /// [`CanonicalError::DigestMismatch`] when they are not.
    pub fn verify(&self, bytes: &[u8]) -> Result<(), CanonicalError> {
        if *self == Self::of(bytes) {
            Ok(())
        } else {
            Err(CanonicalError::DigestMismatch)
        }
    }

    fn parse_with(
        text: &str,
        body_len: usize,
        decode: impl Fn(&str) -> Option<Vec<u8>>,
        render: impl Fn(&Self) -> String,
    ) -> Result<Self, CanonicalError> {
        let body = text
            .strip_prefix(PREFIX)
            .filter(|body| body.len() == body_len)
            .ok_or(CanonicalError::MalformedDigest)?;
        let bytes: [u8; 32] = decode(body)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(CanonicalError::MalformedDigest)?;
        let digest = Self(bytes);
        // A decoder that tolerated non-zero padding bits would give four strings one digest.
        if render(&digest) == text {
            Ok(digest)
        } else {
            Err(CanonicalError::MalformedDigest)
        }
    }
}

impl PartialEq for Sha256Digest {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl Eq for Sha256Digest {}

impl core::hash::Hash for Sha256Digest {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl fmt::Debug for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Sha256Digest({})", self.to_pin())
    }
}

#[cfg(test)]
mod tests {
    use super::Sha256Digest;
    use crate::CanonicalError;

    const EMPTY_SRI: &str = "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=";

    #[test]
    fn the_empty_input_has_the_w3c_digest() {
        assert_eq!(Sha256Digest::of(b"").to_sri(), EMPTY_SRI);
    }

    #[test]
    fn both_spellings_round_trip() {
        let digest = Sha256Digest::of(b"core");
        assert_eq!(Sha256Digest::parse_sri(&digest.to_sri()).ok(), Some(digest));
        assert_eq!(Sha256Digest::parse_pin(&digest.to_pin()).ok(), Some(digest));
        assert_eq!(digest.to_pin().len(), 7 + 43);
    }

    #[test]
    fn a_pin_refuses_the_standard_alphabet_and_padding() {
        let digest = Sha256Digest::of(b"");
        assert!(matches!(
            Sha256Digest::parse_pin(&digest.to_sri()),
            Err(CanonicalError::MalformedDigest)
        ));
    }

    #[test]
    fn nonzero_padding_bits_are_a_second_spelling_and_refused() {
        let pin = Sha256Digest::of(b"").to_pin();
        let last = pin.chars().last().map(|character| match character {
            'U' => 'V',
            other => other,
        });
        let mut tampered: String = pin.chars().take(pin.len() - 1).collect();
        tampered.extend(last);
        assert_ne!(tampered, pin);
        assert!(matches!(
            Sha256Digest::parse_pin(&tampered),
            Err(CanonicalError::MalformedDigest)
        ));
    }

    #[test]
    fn sri_options_and_second_hashes_are_refused() {
        for text in [
            format!("{EMPTY_SRI}?opt"),
            format!("{EMPTY_SRI} {EMPTY_SRI}"),
            EMPTY_SRI.replace("sha256-", "sha384-"),
            EMPTY_SRI.trim_end_matches('=').to_owned(),
        ] {
            assert!(
                Sha256Digest::parse_sri(&text).is_err(),
                "{text} was accepted"
            );
        }
    }

    #[test]
    fn the_bare_spelling_round_trips_and_refuses_padding_bits() {
        let digest = Sha256Digest::of(b"");
        assert_eq!(
            digest.to_base64url(),
            "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU"
        );
        assert_eq!(
            Sha256Digest::parse_base64url(&digest.to_base64url()).ok(),
            Some(digest)
        );
        for other in [
            "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFV",
            "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU",
            "sha256-47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU",
        ] {
            assert!(matches!(
                Sha256Digest::parse_base64url(other),
                Err(CanonicalError::MalformedDigest)
            ));
        }
    }

    #[test]
    fn verify_refuses_other_bytes() {
        let digest = Sha256Digest::of(b"one");
        assert!(digest.verify(b"one").is_ok());
        assert!(matches!(
            digest.verify(b"two"),
            Err(CanonicalError::DigestMismatch)
        ));
    }
}
