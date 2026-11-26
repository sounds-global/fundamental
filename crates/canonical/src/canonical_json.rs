use crate::canonical_writer::write_string;
use crate::utf16_order::utf16_cmp;
use std::collections::BTreeMap;

/// A value with exactly one RFC 8785 encoding, written without an error path.
#[diagnostic::on_unimplemented(
    message = "`{Self}` has no single canonical JSON encoding",
    note = "a float has none in RFC 8785's integer profile: carry a decimal as a string, or its exact bits (`f32::to_bits`) as an integer",
    note = "a 64-bit integer may exceed ±(2^53-1): convert it with `SafeInteger::try_from`"
)]
pub trait CanonicalJson {
    /// Append this value's canonical encoding to `out`.
    fn write_canonical_json(&self, out: &mut Vec<u8>);

    /// This value's canonical encoding.
    #[must_use]
    fn to_canonical_json(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_canonical_json(&mut out);
        out
    }
}

impl CanonicalJson for str {
    fn write_canonical_json(&self, out: &mut Vec<u8>) {
        write_string(out, self);
    }
}

impl CanonicalJson for String {
    fn write_canonical_json(&self, out: &mut Vec<u8>) {
        write_string(out, self);
    }
}

impl CanonicalJson for bool {
    fn write_canonical_json(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(if *self { b"true" } else { b"false" });
    }
}

macro_rules! narrow_integer {
    ($($integer:ty),*) => {$(
        impl CanonicalJson for $integer {
            fn write_canonical_json(&self, out: &mut Vec<u8>) {
                write_decimal(out, i64::from(*self));
            }
        }
    )*};
}

narrow_integer!(u8, u16, u32, i8, i16, i32);

impl<T: CanonicalJson + ?Sized> CanonicalJson for &T {
    fn write_canonical_json(&self, out: &mut Vec<u8>) {
        (**self).write_canonical_json(out);
    }
}

impl<T: CanonicalJson + ?Sized> CanonicalJson for Box<T> {
    fn write_canonical_json(&self, out: &mut Vec<u8>) {
        (**self).write_canonical_json(out);
    }
}

impl<T: CanonicalJson> CanonicalJson for [T] {
    fn write_canonical_json(&self, out: &mut Vec<u8>) {
        out.push(b'[');
        for (index, element) in self.iter().enumerate() {
            if index > 0 {
                out.push(b',');
            }
            element.write_canonical_json(out);
        }
        out.push(b']');
    }
}

impl<T: CanonicalJson, const N: usize> CanonicalJson for [T; N] {
    fn write_canonical_json(&self, out: &mut Vec<u8>) {
        self.as_slice().write_canonical_json(out);
    }
}

impl<T: CanonicalJson> CanonicalJson for Vec<T> {
    fn write_canonical_json(&self, out: &mut Vec<u8>) {
        self.as_slice().write_canonical_json(out);
    }
}

/// Re-sorted by UTF-16 code unit: a `BTreeMap`'s UTF-8 order differs above U+E000.
impl<T: CanonicalJson> CanonicalJson for BTreeMap<String, T> {
    fn write_canonical_json(&self, out: &mut Vec<u8>) {
        let mut members: Vec<(&String, &T)> = self.iter().collect();
        members.sort_by(|(left, _), (right, _)| utf16_cmp(left, right));
        out.push(b'{');
        for (index, (name, value)) in members.into_iter().enumerate() {
            if index > 0 {
                out.push(b',');
            }
            write_string(out, name);
            out.push(b':');
            value.write_canonical_json(out);
        }
        out.push(b'}');
    }
}

pub(crate) fn write_decimal(out: &mut Vec<u8>, value: i64) {
    if value < 0 {
        out.push(b'-');
    }
    let mut magnitude = value.unsigned_abs();
    let mut digits = [0_u8; 20];
    let mut start = digits.len();
    loop {
        start -= 1;
        digits[start] = b'0' + (magnitude % 10) as u8;
        magnitude /= 10;
        if magnitude == 0 {
            break;
        }
    }
    out.extend_from_slice(&digits[start..]);
}

#[cfg(test)]
mod tests {
    use super::CanonicalJson;
    use std::collections::BTreeMap;

    #[test]
    fn integers_have_their_one_decimal_spelling() {
        assert_eq!(0_u8.to_canonical_json(), b"0");
        assert_eq!(i32::MIN.to_canonical_json(), b"-2147483648");
        assert_eq!(u32::MAX.to_canonical_json(), b"4294967295");
        assert_eq!((-7_i8).to_canonical_json(), b"-7");
    }

    #[test]
    fn strings_are_escaped_by_the_writers_escaper() {
        assert_eq!(
            "a\"\\\u{1}é".to_canonical_json(),
            "\"a\\\"\\\\\\u0001é\"".as_bytes()
        );
    }

    #[test]
    fn a_map_is_written_in_utf16_order() {
        let mut map = BTreeMap::new();
        map.insert("\u{E000}".to_owned(), true);
        map.insert("\u{10000}".to_owned(), false);
        assert_eq!(
            map.to_canonical_json(),
            "{\"\u{10000}\":false,\"\u{E000}\":true}".as_bytes()
        );
        assert_eq!(
            map.to_canonical_json(),
            crate::to_canonical_vec(&map).expect("booleans are representable")
        );
    }

    #[test]
    fn sequences_keep_their_order() {
        assert_eq!(vec![3_u8, 1, 2].to_canonical_json(), b"[3,1,2]");
        assert_eq!(Vec::<u8>::new().to_canonical_json(), b"[]");
        assert_eq!(["b", "a"].to_canonical_json(), br#"["b","a"]"#);
    }
}
