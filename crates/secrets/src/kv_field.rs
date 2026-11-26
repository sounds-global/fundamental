//! [`kv_field`] — one field of a key-value (version 2) read, and nothing else.

use std::fmt;
use std::marker::PhantomData;

use serde::Deserializer;
use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, Visitor};

/// The string at `data.data.<key>` of a KV v2 read's body, other fields skipped undecoded.
///
/// # Errors
/// A [`serde_json::Error`] when the body is not JSON, or `data.data.<key>` is not a string.
pub(crate) fn kv_field(body: &[u8], key: &str) -> Result<Option<String>, serde_json::Error> {
    pluck(body, &["data", "data", key], PhantomData::<String>)
}

/// What `leaf` decodes at the end of `path` in `body`, or `None` when a step is missing or `null`.
///
/// # Errors
/// A [`serde_json::Error`] when the body is not JSON, or `leaf` refuses what it finds.
pub(crate) fn pluck<'de, S>(
    body: &'de [u8],
    path: &[&str],
    leaf: S,
) -> Result<Option<S::Value>, serde_json::Error>
where
    S: DeserializeSeed<'de>,
{
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let found = Pluck { path, leaf }.deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(found)
}

/// Descends one map key per `path` element and hands what is at the end of it to `leaf`.
struct Pluck<'a, S> {
    path: &'a [&'a str],
    leaf: S,
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Pluck<'_, S> {
    type Value = Option<S::Value>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        match self.path.split_first() {
            None => self.leaf.deserialize(deserializer).map(Some),
            Some((wanted, rest)) => deserializer.deserialize_any(PluckMap {
                wanted,
                rest,
                leaf: self.leaf,
            }),
        }
    }
}

struct PluckMap<'a, S> {
    wanted: &'a str,
    rest: &'a [&'a str],
    leaf: S,
}

impl<'de, S: DeserializeSeed<'de>> Visitor<'de> for PluckMap<'_, S> {
    type Value = Option<S::Value>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "an object holding `{}`", self.wanted)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut leaf = Some(self.leaf);
        let mut found = None;
        while let Some(name) = map.next_key::<std::borrow::Cow<'de, str>>()? {
            match leaf.take() {
                Some(seed) if name == self.wanted => {
                    found = map.next_value_seed(Pluck {
                        path: self.rest,
                        leaf: seed,
                    })?;
                }
                unused => {
                    leaf = unused;
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(found)
    }

    // A `null` where an object was expected (a deleted version) holds no field.
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::kv_field;

    #[test]
    fn the_named_field_is_read_and_its_neighbours_are_not() {
        let body = br#"{"request_id":"r","data":{"data":{"other":"not-me","password":"pw","n":{"deep":[1,2]}},"metadata":{"version":3}}}"#;
        assert_eq!(
            kv_field(body, "password").expect("json").as_deref(),
            Some("pw")
        );
    }

    #[test]
    fn a_missing_field_or_a_deleted_version_is_none() {
        assert_eq!(
            kv_field(br#"{"data":{"data":{"other":"x"}}}"#, "password").expect("json"),
            None
        );
        assert_eq!(
            kv_field(br#"{"data":{"data":null}}"#, "password").expect("json"),
            None
        );
        assert_eq!(
            kv_field(br#"{"errors":[]}"#, "password").expect("json"),
            None
        );
    }

    #[test]
    fn a_field_that_is_not_a_string_or_a_body_that_is_not_json_is_an_error() {
        assert!(kv_field(br#"{"data":{"data":{"password":42}}}"#, "password").is_err());
        assert!(kv_field(b"<html>", "password").is_err());
        assert!(kv_field(br#"{"data":{"data":{}}} trailing"#, "password").is_err());
    }
}
