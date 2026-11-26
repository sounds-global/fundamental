//! [`KvFields`] — the named fields of one key-value read, wiped when dropped.

use std::fmt;

use serde::Deserializer;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, Visitor};
use zeroize::Zeroizing;

use crate::kv_field::pluck;

/// The named fields of a key-value (version 2) read, each wiped when dropped; `Debug` prints names only.
pub struct KvFields {
    fields: Vec<(String, Zeroizing<String>)>,
}

impl KvFields {
    /// The value of the field `key`, if the read found it.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    /// The names of the fields the read found.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.fields.iter().map(|(name, _)| name.as_str())
    }

    /// The fields named `keys` at `data.data` of a read's `body`, every other field skipped
    /// undecoded; `None` for a deleted or destroyed version.
    ///
    /// # Errors
    /// A [`serde_json::Error`] when the body is not JSON. Its message never quotes a value.
    pub(crate) fn parse(body: &[u8], keys: &[&str]) -> Result<Option<Self>, serde_json::Error> {
        pluck(body, &["data", "data"], Collect { keys }).map(Option::flatten)
    }
}

impl fmt::Debug for KvFields {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KvFields")
            .field("keys", &self.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Takes the string fields named `keys` from a map, skipping everything else.
struct Collect<'k> {
    keys: &'k [&'k str],
}

impl<'de> DeserializeSeed<'de> for Collect<'_> {
    type Value = Option<KvFields>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Collect<'_> {
    type Value = Option<KvFields>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a secret's fields")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut fields = Vec::with_capacity(self.keys.len());
        while let Some(name) = map.next_key::<std::borrow::Cow<'de, str>>()? {
            let wanted = self.keys.contains(&name.as_ref())
                && !fields.iter().any(|(taken, _): &(String, _)| *taken == name);
            if !wanted {
                map.next_value::<IgnoredAny>()?;
                continue;
            }
            let value = map.next_value::<Option<StringOnly>>()?.and_then(|v| v.0);
            if let Some(value) = value {
                fields.push((name.into_owned(), value));
            }
        }
        Ok(Some(KvFields { fields }))
    }

    // `data.data: null` is a deleted or destroyed version.
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(None)
    }
}

/// A string moved straight into a wiped buffer; anything else is skipped and reads as absent.
struct StringOnly(Option<Zeroizing<String>>);

impl<'de> serde::Deserialize<'de> for StringOnly {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StringOnlyVisitor)
    }
}

struct StringOnlyVisitor;

impl<'de> Visitor<'de> for StringOnlyVisitor {
    type Value = StringOnly;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a string")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StringOnly(Some(Zeroizing::new(value.to_owned()))))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StringOnly(Some(Zeroizing::new(value))))
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(StringOnly(None))
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(StringOnly(None))
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(StringOnly(None))
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(StringOnly(None))
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(StringOnly(None))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(StringOnly(None))
    }
}

#[cfg(test)]
mod tests {
    use super::KvFields;

    #[test]
    fn only_the_named_string_fields_are_read() {
        let body = br#"{"data":{"data":{"user":"u","password":"p","other":"o","n":42,"_entry":"e"},"metadata":{"version":2}}}"#;
        let fields = KvFields::parse(body, &["user", "password", "n", "missing"])
            .expect("json")
            .expect("data");
        assert_eq!(fields.get("user"), Some("u"));
        assert_eq!(fields.get("password"), Some("p"));
        assert_eq!(fields.get("other"), None, "not asked for");
        assert_eq!(fields.get("n"), None, "not a string");
        assert_eq!(fields.get("missing"), None);
    }

    #[test]
    fn a_deleted_version_holds_no_fields() {
        let body = br#"{"data":{"data":null,"metadata":{"deletion_time":"x"}}}"#;
        assert!(KvFields::parse(body, &["user"]).expect("json").is_none());
    }

    #[test]
    fn a_repeated_name_keeps_the_first_value() {
        let body = br#"{"data":{"data":{"k":"first","k":"second"}}}"#;
        let fields = KvFields::parse(body, &["k"]).expect("json").expect("data");
        assert_eq!(fields.get("k"), Some("first"));
    }

    #[test]
    fn debug_prints_the_names_and_never_a_value() {
        let body = br#"{"data":{"data":{"password":"hunter2"}}}"#;
        let fields = KvFields::parse(body, &["password"])
            .expect("json")
            .expect("data");
        let printed = format!("{fields:?}");
        assert!(printed.contains("password") && !printed.contains("hunter2"));
    }
}
