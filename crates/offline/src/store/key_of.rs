//! [`KeyOf`]: a record's key as a hash-map key.

use std::hash::{Hash, Hasher};

use crate::db::Value;

/// A record's key [`Value`], hashable so translations can be grouped by it.
///
/// Compares a `Real` by its bits; `Value` stays without `Eq` because of NaN.
#[derive(Debug)]
pub(super) struct KeyOf(pub(super) Value);

impl PartialEq for KeyOf {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Value::Null, Value::Null) => true,
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Real(a), Value::Real(b)) => a.to_bits() == b.to_bits(),
            (Value::Text(a), Value::Text(b)) => a == b,
            (Value::Blob(a), Value::Blob(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for KeyOf {}

impl Hash for KeyOf {
    fn hash<H: Hasher>(&self, state: &mut H) {
        core::mem::discriminant(&self.0).hash(state);
        match &self.0 {
            Value::Null => {}
            Value::Integer(i) => i.hash(state),
            Value::Real(r) => r.to_bits().hash(state),
            Value::Text(t) => t.hash(state),
            Value::Blob(b) => b.hash(state),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn equal_keys_find_each_other_and_different_kinds_do_not() {
        let mut map = HashMap::new();
        map.insert(KeyOf(Value::Integer(7)), "seven");
        map.insert(KeyOf(Value::Text("7".to_owned())), "text");
        map.insert(KeyOf(Value::Real(f64::NAN)), "nan");

        assert_eq!(map.get(&KeyOf(Value::Integer(7))), Some(&"seven"));
        assert_eq!(map.get(&KeyOf(Value::Text("7".to_owned()))), Some(&"text"));
        assert_eq!(map.get(&KeyOf(Value::Real(f64::NAN))), Some(&"nan"));
        assert_eq!(map.get(&KeyOf(Value::Integer(8))), None);
        assert_eq!(map.get(&KeyOf(Value::Null)), None);
    }
}
