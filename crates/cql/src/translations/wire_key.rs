//! [`WireKey`]: a record's clustering key as YCQL returned it, usable as a map key.

use std::hash::{Hash, Hasher};

use scylla::value::CqlValue;

/// A record's clustering key columns, as the driver decoded them, hashable.
///
/// The driver's [`CqlValue`] has neither `Eq` nor `Hash` (it can hold a float), so floats
/// compare by their bits.
#[derive(Debug, Clone)]
pub(crate) struct WireKey(pub(crate) Vec<Option<CqlValue>>);

impl WireKey {
    /// The first clustering column's value: the one a page's range is bounded by.
    pub(crate) fn first(&self) -> Option<&CqlValue> {
        self.0.first().and_then(Option::as_ref)
    }
}

impl PartialEq for WireKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(&other.0).all(|(a, b)| match (a, b) {
                (None, None) => true,
                (Some(a), Some(b)) => same(a, b),
                _ => false,
            })
    }
}

impl Eq for WireKey {}

impl Hash for WireKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.len().hash(state);
        for column in &self.0 {
            match column {
                None => 0_u8.hash(state),
                Some(value) => hash_value(value, state),
            }
        }
    }
}

/// Two decoded values are one key value: floats by their bits, the rest as the driver says.
fn same(a: &CqlValue, b: &CqlValue) -> bool {
    match (a, b) {
        (CqlValue::Float(a), CqlValue::Float(b)) => a.to_bits() == b.to_bits(),
        (CqlValue::Double(a), CqlValue::Double(b)) => a.to_bits() == b.to_bits(),
        _ => a == b,
    }
}

/// Hashes a value's kind and its content; a kind hashed alone would match a page in quadratic
/// time.
fn hash_value<H: Hasher>(value: &CqlValue, state: &mut H) {
    core::mem::discriminant(value).hash(state);
    match value {
        CqlValue::Text(s) | CqlValue::Ascii(s) => s.hash(state),
        CqlValue::BigInt(i) => i.hash(state),
        CqlValue::Int(i) => i.hash(state),
        CqlValue::SmallInt(i) => i.hash(state),
        CqlValue::TinyInt(i) => i.hash(state),
        CqlValue::Boolean(b) => b.hash(state),
        CqlValue::Uuid(u) => u.hash(state),
        CqlValue::Timestamp(t) => t.0.hash(state),
        CqlValue::Date(d) => d.0.hash(state),
        CqlValue::Time(t) => t.0.hash(state),
        CqlValue::Blob(b) => b.hash(state),
        CqlValue::Timeuuid(t) => t.hash(state),
        CqlValue::Varint(v) => v.hash(state),
        CqlValue::Inet(ip) => ip.hash(state),
        CqlValue::Float(f) => f.to_bits().hash(state),
        CqlValue::Double(f) => f.to_bits().hash(state),
        CqlValue::Decimal(d) => d.as_signed_be_bytes_slice_and_exponent().hash(state),
        CqlValue::Duration(d) => (d.months, d.days, d.nanoseconds).hash(state),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use scylla::value::{CqlTimeuuid, CqlVarint};

    use super::*;

    fn key(values: Vec<CqlValue>) -> WireKey {
        WireKey(values.into_iter().map(Some).collect())
    }

    #[test]
    fn the_same_key_finds_its_entry_and_another_does_not() {
        let id = uuid::Uuid::new_v4();
        let mut map = HashMap::new();
        map.insert(key(vec![CqlValue::BigInt(2)]), "two");
        map.insert(
            key(vec![CqlValue::Text("a".into()), CqlValue::Uuid(id)]),
            "text and uuid",
        );
        map.insert(
            key(vec![CqlValue::Timeuuid(CqlTimeuuid::from(id))]),
            "timeuuid",
        );
        map.insert(key(vec![CqlValue::Double(f64::NAN)]), "nan");

        assert_eq!(map.get(&key(vec![CqlValue::BigInt(2)])), Some(&"two"));
        assert_eq!(
            map.get(&key(vec![CqlValue::Text("a".into()), CqlValue::Uuid(id)])),
            Some(&"text and uuid")
        );
        assert_eq!(
            map.get(&key(vec![CqlValue::Timeuuid(CqlTimeuuid::from(id))])),
            Some(&"timeuuid")
        );
        assert_eq!(
            map.get(&key(vec![CqlValue::Double(f64::NAN)])),
            Some(&"nan")
        );
        assert_eq!(map.get(&key(vec![CqlValue::BigInt(3)])), None);
        assert_eq!(map.get(&key(vec![CqlValue::Int(2)])), None, "another kind");
        assert_eq!(map.get(&WireKey(vec![None])), None);
    }

    #[test]
    fn different_keys_of_every_hashed_kind_hash_apart() {
        use std::hash::BuildHasher;

        let state = std::hash::RandomState::new();
        let hash = |value: CqlValue| state.hash_one(key(vec![value]));
        let pairs = [
            (
                CqlValue::Timeuuid(CqlTimeuuid::from(uuid::Uuid::from_u128(1))),
                CqlValue::Timeuuid(CqlTimeuuid::from(uuid::Uuid::from_u128(2))),
            ),
            (CqlValue::Double(1.5), CqlValue::Double(2.5)),
            (CqlValue::Float(1.5), CqlValue::Float(2.5)),
            (
                CqlValue::Varint(CqlVarint::from_signed_bytes_be(vec![1])),
                CqlValue::Varint(CqlVarint::from_signed_bytes_be(vec![2])),
            ),
            (
                CqlValue::Inet([10, 0, 0, 1].into()),
                CqlValue::Inet([10, 0, 0, 2].into()),
            ),
        ];
        for (a, b) in pairs {
            let label = format!("{a:?} and {b:?}");
            assert_ne!(
                hash(a),
                hash(b),
                "{label} share a hash: a page keyed by them is matched in quadratic time"
            );
        }
    }

    #[test]
    fn a_key_of_no_columns_is_the_one_record_of_its_partition() {
        let mut map = HashMap::new();
        map.insert(WireKey(Vec::new()), "the record");
        assert_eq!(map.get(&WireKey(Vec::new())), Some(&"the record"));
    }
}
