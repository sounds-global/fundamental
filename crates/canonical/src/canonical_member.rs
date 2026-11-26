use crate::CanonicalJson;

/// How a derived struct writes one member: an `Option` only when it holds a value, never `null`.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "`{Self}` has no single canonical JSON encoding",
    note = "a float has none in RFC 8785's integer profile: carry a decimal as a string, or its exact bits (`f32::to_bits`) as an integer",
    note = "a 64-bit integer may exceed ±(2^53-1): convert it with `SafeInteger::try_from`"
)]
pub trait CanonicalMember {
    /// Append `,` when this is not the first member written, then `key` and the value.
    fn write_member(&self, key: &[u8], out: &mut Vec<u8>, first: &mut bool);
}

impl<T: CanonicalJson + ?Sized> CanonicalMember for T {
    fn write_member(&self, key: &[u8], out: &mut Vec<u8>, first: &mut bool) {
        if !core::mem::take(first) {
            out.push(b',');
        }
        out.extend_from_slice(key);
        self.write_canonical_json(out);
    }
}

impl<T: CanonicalJson> CanonicalMember for Option<T> {
    fn write_member(&self, key: &[u8], out: &mut Vec<u8>, first: &mut bool) {
        if let Some(value) = self {
            value.write_member(key, out, first);
        }
    }
}
