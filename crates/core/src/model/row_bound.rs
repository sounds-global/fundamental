#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

use uuid::Uuid;

use super::TimestampPrecision;

/// The canonical, type-tagged bytes one column contributes when a model's encrypted fields are bound to their row.
///
/// The bytes must be identical when the row is written and when it is read back, so each column type states its
/// encoding here, once. Each part leads with a type tag, so equal bytes of different types never bind alike; a
/// `NULL` has no part, so it never binds like an empty string.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be bound to a row",
    label = "this column's type has no canonical row-binding encoding",
    note = "implement `RowBound` for it, or mark the column `#[model(unbound)]` if it is bookkeeping written independently of the model's encrypted values"
)]
pub trait RowBound {
    /// This column value's tagged bytes, or `None` when the value is `NULL`.
    fn row_part(&self, precision: TimestampPrecision) -> Option<Vec<u8>>;
}

const TEXT: u8 = 0x01;
const UUID: u8 = 0x02;
const SIGNED: u8 = 0x03;
const UNSIGNED: u8 = 0x04;
const BOOLEAN: u8 = 0x05;
#[cfg(feature = "chrono")]
const INSTANT: u8 = 0x06;
const BYTES: u8 = 0x07;

fn tagged(tag: u8, bytes: &[u8]) -> Option<Vec<u8>> {
    let mut part = Vec::with_capacity(bytes.len() + 1);
    part.push(tag);
    part.extend_from_slice(bytes);
    Some(part)
}

impl RowBound for str {
    fn row_part(&self, _: TimestampPrecision) -> Option<Vec<u8>> {
        tagged(TEXT, self.as_bytes())
    }
}

impl RowBound for String {
    fn row_part(&self, precision: TimestampPrecision) -> Option<Vec<u8>> {
        self.as_str().row_part(precision)
    }
}

impl RowBound for [u8] {
    fn row_part(&self, _: TimestampPrecision) -> Option<Vec<u8>> {
        tagged(BYTES, self)
    }
}

impl RowBound for Vec<u8> {
    fn row_part(&self, precision: TimestampPrecision) -> Option<Vec<u8>> {
        self.as_slice().row_part(precision)
    }
}

/// A fixed-length byte string binds as a `Vec<u8>`, which is how its column reads back.
impl<const N: usize> RowBound for [u8; N] {
    fn row_part(&self, precision: TimestampPrecision) -> Option<Vec<u8>> {
        self.as_slice().row_part(precision)
    }
}

impl<T: RowBound + ?Sized> RowBound for &T {
    fn row_part(&self, precision: TimestampPrecision) -> Option<Vec<u8>> {
        (**self).row_part(precision)
    }
}

impl<T: RowBound> RowBound for Option<T> {
    fn row_part(&self, precision: TimestampPrecision) -> Option<Vec<u8>> {
        self.as_ref().and_then(|value| value.row_part(precision))
    }
}

impl RowBound for Uuid {
    fn row_part(&self, _: TimestampPrecision) -> Option<Vec<u8>> {
        tagged(UUID, self.as_bytes())
    }
}

impl RowBound for bool {
    fn row_part(&self, _: TimestampPrecision) -> Option<Vec<u8>> {
        tagged(BOOLEAN, &[u8::from(*self)])
    }
}

macro_rules! signed_row_bound {
    ($($ty:ty),*) => {$(
        impl RowBound for $ty {
            fn row_part(&self, _: TimestampPrecision) -> Option<Vec<u8>> {
                tagged(SIGNED, &i64::from(*self).to_le_bytes())
            }
        }
    )*};
}

macro_rules! unsigned_row_bound {
    ($($ty:ty),*) => {$(
        impl RowBound for $ty {
            fn row_part(&self, _: TimestampPrecision) -> Option<Vec<u8>> {
                tagged(UNSIGNED, &u64::from(*self).to_le_bytes())
            }
        }
    )*};
}

signed_row_bound!(i8, i16, i32, i64);
unsigned_row_bound!(u8, u16, u32, u64);

#[cfg(feature = "chrono")]
impl RowBound for chrono::DateTime<chrono::Utc> {
    fn row_part(&self, precision: TimestampPrecision) -> Option<Vec<u8>> {
        let units = match precision {
            TimestampPrecision::Microseconds => self.timestamp_micros(),
            TimestampPrecision::Milliseconds => self.timestamp_millis(),
        };
        tagged(INSTANT, &units.to_le_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::{RowBound, TimestampPrecision};
    use uuid::Uuid;

    const P: TimestampPrecision = TimestampPrecision::Microseconds;

    #[test]
    fn text_is_tagged_utf8_whether_owned_or_borrowed() {
        let owned = String::from("zone-eu");
        let reference: &String = &owned;
        assert_eq!(owned.row_part(P), Some(b"\x01zone-eu".to_vec()));
        assert_eq!("zone-eu".row_part(P), owned.row_part(P));
        assert_eq!(RowBound::row_part(&reference, P), owned.row_part(P));
    }

    #[test]
    fn null_has_no_part_and_an_empty_string_does() {
        assert_eq!(None::<String>.row_part(P), None);
        assert_eq!(Some(String::new()).row_part(P), Some(vec![0x01]));
        assert_eq!(Some(7_i64).row_part(P), 7_i64.row_part(P));
    }

    #[test]
    fn equal_bytes_of_different_types_bind_differently() {
        let id = Uuid::from_u128(0x6162_6364_6566_6768_696a_6b6c_6d6e_6f70);
        let text = String::from_utf8(id.as_bytes().to_vec()).expect("ascii bytes");
        assert_ne!(id.row_part(P), text.row_part(P));
        assert_ne!(1_i64.row_part(P), 1_u64.row_part(P));
    }

    #[test]
    fn bytes_bind_as_themselves_and_never_as_the_text_they_spell() {
        let owned = b"ab".to_vec();
        assert_eq!(owned.row_part(P), Some(vec![0x07, b'a', b'b']));
        assert_eq!(owned.as_slice().row_part(P), owned.row_part(P));
        assert_ne!(owned.row_part(P), "ab".row_part(P));
        assert_eq!(Vec::<u8>::new().row_part(P), Some(vec![0x07]));
        assert_ne!(Vec::<u8>::new().row_part(P), String::new().row_part(P));
    }

    #[test]
    fn a_fixed_length_byte_string_binds_as_the_bytes_it_reads_back_as() {
        let digest = [7_u8; 32];
        assert_eq!(digest.row_part(P), digest.to_vec().row_part(P));
        assert_ne!(digest.row_part(P), [7_u8; 31].row_part(P));
    }

    #[test]
    fn integers_and_booleans_are_fixed_width() {
        assert_eq!(7_i32.row_part(P), 7_i64.row_part(P));
        assert_eq!(7_u16.row_part(P).map(|part| part.len()), Some(9));
        assert_eq!(true.row_part(P), Some(vec![0x05, 1]));
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn an_instant_binds_at_the_wire_precision() {
        use chrono::{TimeZone, Utc};
        let written = Utc
            .timestamp_opt(1_700_000_000, 123_456_789)
            .single()
            .expect("valid");
        let read_back_sql = Utc
            .timestamp_opt(1_700_000_000, 123_456_000)
            .single()
            .expect("valid");
        let read_back_cql = Utc
            .timestamp_opt(1_700_000_000, 123_000_000)
            .single()
            .expect("valid");
        assert_eq!(
            written.row_part(TimestampPrecision::Microseconds),
            read_back_sql.row_part(TimestampPrecision::Microseconds),
            "a nanosecond the SQL wire drops must not change the binding"
        );
        assert_eq!(
            written.row_part(TimestampPrecision::Milliseconds),
            read_back_cql.row_part(TimestampPrecision::Milliseconds),
            "a microsecond the CQL wire drops must not change the binding"
        );
    }
}
