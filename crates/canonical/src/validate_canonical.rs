use crate::{CanonicalError, Limits, Profile, from_canonical_slice};
use serde::de::IgnoredAny;

/// Check that `bytes` are a canonical document, building no value from them.
///
/// # Errors
///
/// The [`CanonicalError`] [`from_canonical_slice`] would return.
pub fn validate_canonical(
    bytes: &[u8],
    limits: &Limits,
    profile: Profile,
) -> Result<(), CanonicalError> {
    from_canonical_slice::<IgnoredAny>(bytes, limits, profile).map(|_| ())
}
