use crate::{CanonicalError, Limits, Profile, Sha256Digest, from_normal_slice};
use serde::{Deserialize, Serialize};

/// Read a document only after its bytes have matched the digest the caller already trusts.
///
/// # Errors
///
/// [`CanonicalError::TooLarge`], [`CanonicalError::DigestMismatch`], or what
/// [`from_normal_slice`] would return.
pub fn from_digested_slice<'de, T: Deserialize<'de> + Serialize>(
    bytes: &'de [u8],
    expected: &Sha256Digest,
    limits: &Limits,
    profile: Profile,
) -> Result<T, CanonicalError> {
    limits.admit_len(bytes.len())?;
    expected.verify(bytes)?;
    from_normal_slice(bytes, limits, profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserializer, Serializer};
    use std::cell::Cell;

    thread_local! {
        static DESERIALIZED: Cell<u32> = const { Cell::new(0) };
    }

    /// Counts its deserialisations, so a test sees whether parsing began.
    #[derive(Debug)]
    struct Probe;

    impl<'de> Deserialize<'de> for Probe {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            DESERIALIZED.with(|count| count.set(count.get() + 1));
            serde::de::IgnoredAny::deserialize(deserializer)?;
            Ok(Self)
        }
    }

    impl Serialize for Probe {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serde::ser::SerializeMap::end(serializer.serialize_map(Some(0))?)
        }
    }

    fn parses_of(bytes: &[u8], expected: &Sha256Digest) -> (Result<Probe, CanonicalError>, u32) {
        DESERIALIZED.with(|count| count.set(0));
        let result =
            from_digested_slice::<Probe>(bytes, expected, &Limits::DOCUMENT, Profile::Verifier);
        (result, DESERIALIZED.with(Cell::get))
    }

    #[test]
    fn a_verifier_never_parses_before_the_digest_matches() {
        let (result, parses) = parses_of(b" {}", &Sha256Digest::of(b"{}"));
        assert!(
            matches!(result, Err(CanonicalError::DigestMismatch)),
            "{result:?}"
        );
        assert_eq!(parses, 0, "bytes under a wrong digest reached the parser");

        let (result, parses) = parses_of(b"{}", &Sha256Digest::of(b"{}"));
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            parses, 1,
            "the control must reach the parser, or the probe proves nothing"
        );
    }

    #[test]
    fn a_malformed_document_under_its_own_digest_is_refused_by_the_parser() {
        let malformed = b" {}";
        let (result, parses) = parses_of(malformed, &Sha256Digest::of(malformed));
        assert!(
            matches!(result, Err(CanonicalError::Whitespace { offset: 0 })),
            "{result:?}"
        );
        assert_eq!(parses, 1);
    }

    #[test]
    fn an_oversized_input_is_refused_before_it_is_hashed() {
        let oversized = vec![b'0'; Limits::DOCUMENT.max_bytes + 1];
        for expected in [Sha256Digest::of(b"{}"), Sha256Digest::of(&oversized)] {
            let (result, parses) = parses_of(&oversized, &expected);
            assert!(
                matches!(
                    result,
                    Err(CanonicalError::TooLarge { len, max })
                        if len == oversized.len() && max == Limits::DOCUMENT.max_bytes
                ),
                "{result:?}"
            );
            assert_eq!(parses, 0);
        }
    }
}
