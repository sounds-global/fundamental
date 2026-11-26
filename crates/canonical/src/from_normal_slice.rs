use crate::{CanonicalError, Limits, Profile, from_canonical_slice, to_canonical_vec};
use serde::{Deserialize, Serialize};

/// Read `bytes` as `T` and require them to be the bytes `T` itself writes.
///
/// This refuses canonical second spellings such as a default written out or `null` for an absent member.
///
/// # Errors
///
/// The [`CanonicalError`] [`from_canonical_slice`] would return, or
/// [`CanonicalError::NotNormal`] when the bytes are canonical but not `T`'s own encoding.
pub fn from_normal_slice<'de, T: Deserialize<'de> + Serialize>(
    bytes: &'de [u8],
    limits: &Limits,
    profile: Profile,
) -> Result<T, CanonicalError> {
    let value: T = from_canonical_slice(bytes, limits, profile)?;
    if to_canonical_vec(&value)? != bytes {
        return Err(CanonicalError::NotNormal);
    }
    Ok(value)
}
