use crate::reader::Reader;
use crate::strict_deserializer::StrictDeserializer;
use crate::{CanonicalError, Limits, Profile};
use serde::Deserialize;

/// Read `bytes` as `T`, accepting them only if they are already in RFC 8785 canonical form.
///
/// ```
/// use fundamental_canonical::{Limits, Profile, from_canonical_slice};
///
/// let value: std::collections::BTreeMap<String, i64> =
///     from_canonical_slice(br#"{"a":1,"b":2}"#, &Limits::DOCUMENT, Profile::Verifier)?;
/// assert_eq!(value["b"], 2);
/// assert!(from_canonical_slice::<std::collections::BTreeMap<String, i64>>(
///     br#"{"b":2,"a":1}"#, &Limits::DOCUMENT, Profile::Verifier,
/// ).is_err());
/// # Ok::<(), fundamental_canonical::CanonicalError>(())
/// ```
///
/// # Errors
///
/// A [`CanonicalError`] naming the first refusal and the byte offset it sits at, or
/// [`CanonicalError::Shape`] when canonical bytes do not have the shape `T` expects.
pub fn from_canonical_slice<'de, T: Deserialize<'de>>(
    bytes: &'de [u8],
    limits: &Limits,
    profile: Profile,
) -> Result<T, CanonicalError> {
    let mut deserializer = StrictDeserializer::new(Reader::new(bytes, *limits, profile)?);
    let value = T::deserialize(&mut deserializer)?;
    deserializer.finish()?;
    Ok(value)
}
