use crate::CanonicalError;
use crate::canonical_writer::CanonicalWriter;
use serde::Serialize;

/// Encode `value` as its one RFC 8785 byte sequence.
///
/// ```
/// # use serde::Serialize;
/// #[derive(Serialize)]
/// struct Label { zeta: u8, alpha: &'static str }
///
/// let bytes = fundamental_canonical::to_canonical_vec(&Label { zeta: 1, alpha: "a\u{1}" })?;
/// assert_eq!(bytes, br#"{"alpha":"a\u0001","zeta":1}"#);
/// # Ok::<(), fundamental_canonical::CanonicalError>(())
/// ```
///
/// # Errors
///
/// [`CanonicalError::Unrepresentable`] when `value` has no exact canonical encoding.
pub fn to_canonical_vec<T: ?Sized + Serialize>(value: &T) -> Result<Vec<u8>, CanonicalError> {
    let mut out = Vec::new();
    value.serialize(&mut CanonicalWriter::new(&mut out))?;
    Ok(out)
}
