use serde::de::{Error as _, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The rendered values of a row's key columns, in key order; never empty by construction.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordKey {
    first: String,
    rest: Vec<String>,
}

impl RecordKey {
    /// A single-column key.
    #[must_use]
    pub fn single(value: impl Into<String>) -> Self {
        Self {
            first: value.into(),
            rest: Vec::new(),
        }
    }

    /// A key of two or more columns, in key order.
    #[must_use]
    pub fn composite(
        first: impl Into<String>,
        rest: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            first: first.into(),
            rest: rest.into_iter().map(Into::into).collect(),
        }
    }

    /// The components, in key order. Never empty.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.first.as_str()).chain(self.rest.iter().map(String::as_str))
    }

    /// How many columns address this row. Never zero.
    #[must_use]
    pub fn len(&self) -> usize {
        1 + self.rest.len()
    }

    /// The lone component of a single-column key, or `None` for a composite key.
    #[must_use]
    pub fn as_single(&self) -> Option<&str> {
        self.rest.is_empty().then_some(self.first.as_str())
    }

    /// Always `false`: a key has at least one component by construction.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The netstring rendering (`{len}:{bytes},`) used where a whole key must occupy one text cell.
    ///
    /// Self-delimiting, so a concatenation of components parses back uniquely.
    #[must_use]
    pub fn to_netstring(&self) -> String {
        let mut out = String::new();
        for component in self.components() {
            out.push_str(&component.len().to_string());
            out.push(':');
            out.push_str(component);
            out.push(',');
        }
        out
    }

    /// Parses [`to_netstring`](Self::to_netstring).
    ///
    /// # Errors
    /// Returns the offending text when the input is not a netstring sequence, or encodes
    /// no components at all.
    pub fn from_netstring(text: &str) -> Result<Self, String> {
        let mut components = Vec::new();
        let bytes = text.as_bytes();
        let mut at = 0usize;

        while at < bytes.len() {
            let colon = text[at..]
                .find(':')
                .ok_or_else(|| format!("no length prefix at byte {at} of `{text}`"))?
                + at;
            let len: usize = text[at..colon]
                .parse()
                .map_err(|_| format!("`{}` is not a length", &text[at..colon]))?;
            let start = colon + 1;
            let end = start + len;
            if end >= bytes.len() || bytes[end] != b',' {
                return Err(format!("component at byte {start} is not terminated"));
            }
            components.push(
                text.get(start..end)
                    .ok_or_else(|| format!("component at byte {start} is not valid UTF-8"))?
                    .to_owned(),
            );
            at = end + 1;
        }

        let mut components = components.into_iter();
        let first = components
            .next()
            .ok_or_else(|| format!("`{text}` encodes no key components"))?;
        Ok(Self {
            first,
            rest: components.collect(),
        })
    }
}

/// Renders for a human: a single-column key is its bare component, a composite one
/// is a parenthesised list.
///
/// Not a storage form and never parsed; use [`to_netstring`](RecordKey::to_netstring) for that.
impl std::fmt::Display for RecordKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.rest.is_empty() {
            return f.write_str(&self.first);
        }
        f.write_str("(")?;
        for (at, component) in self.components().enumerate() {
            if at > 0 {
                f.write_str(", ")?;
            }
            f.write_str(component)?;
        }
        f.write_str(")")
    }
}

/// A key equals a bare string exactly when it is that one column.
impl PartialEq<str> for RecordKey {
    fn eq(&self, other: &str) -> bool {
        self.as_single() == Some(other)
    }
}

impl PartialEq<&str> for RecordKey {
    fn eq(&self, other: &&str) -> bool {
        self.as_single() == Some(*other)
    }
}

impl PartialEq<String> for RecordKey {
    fn eq(&self, other: &String) -> bool {
        self.as_single() == Some(other.as_str())
    }
}

impl PartialEq<RecordKey> for String {
    fn eq(&self, other: &RecordKey) -> bool {
        other.as_single() == Some(self.as_str())
    }
}

impl PartialEq<RecordKey> for str {
    fn eq(&self, other: &RecordKey) -> bool {
        other.as_single() == Some(self)
    }
}

impl PartialEq<RecordKey> for &str {
    fn eq(&self, other: &RecordKey) -> bool {
        other.as_single() == Some(*self)
    }
}

impl From<String> for RecordKey {
    fn from(value: String) -> Self {
        Self::single(value)
    }
}

impl From<&str> for RecordKey {
    fn from(value: &str) -> Self {
        Self::single(value)
    }
}

impl From<&String> for RecordKey {
    fn from(value: &String) -> Self {
        Self::single(value.as_str())
    }
}

/// A single-column key is a bare JSON string; a composite one is an array.
///
/// The two are disjoint JSON kinds, so the encoding is injective with no separator to forge.
impl Serialize for RecordKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.rest.is_empty() {
            serializer.serialize_str(&self.first)
        } else {
            serializer.collect_seq(self.components())
        }
    }
}

impl<'de> Deserialize<'de> for RecordKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct KeyVisitor;

        impl<'de> Visitor<'de> for KeyVisitor {
            type Value = RecordKey;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a record key: a string, or a non-empty array of strings")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(RecordKey::single(value))
            }

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut components: Vec<String> = Vec::new();
                while let Some(component) = seq.next_element()? {
                    components.push(component);
                }
                let mut components = components.into_iter();
                // A one-element array normalises to the bare-string form, so one key has one spelling.
                let first = components
                    .next()
                    .ok_or_else(|| A::Error::custom("a record key has no empty form"))?;
                Ok(RecordKey {
                    first,
                    rest: components.collect(),
                })
            }
        }

        deserializer.deserialize_any(KeyVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::RecordKey;

    #[test]
    fn a_single_component_key_serialises_as_a_bare_string() {
        let json = serde_json::to_string(&RecordKey::single("row-1")).expect("serialise");
        assert_eq!(json, "\"row-1\"", "arity 1 must not change on the wire");
    }

    #[test]
    fn a_composite_key_serialises_as_an_array() {
        let key = RecordKey::composite("eu", ["owner-1", "row-1"]);
        let json = serde_json::to_string(&key).expect("serialise");
        assert_eq!(json, r#"["eu","owner-1","row-1"]"#);
    }

    #[test]
    fn a_key_round_trips_at_every_arity() {
        for key in [
            RecordKey::single("row-1"),
            RecordKey::composite("eu", ["row-1"]),
            RecordKey::composite("eu", ["owner-1", "row-1"]),
        ] {
            let json = serde_json::to_string(&key).expect("serialise");
            let back: RecordKey = serde_json::from_str(&json).expect("deserialise");
            assert_eq!(back, key);
        }
    }

    #[test]
    fn a_one_element_array_reads_as_the_bare_string_form() {
        let from_array: RecordKey = serde_json::from_str(r#"["row-1"]"#).expect("deserialise");
        assert_eq!(from_array, RecordKey::single("row-1"));
        assert_eq!(
            serde_json::to_string(&from_array).expect("serialise"),
            "\"row-1\"",
            "one key must have one spelling, or it signs two sets of bytes"
        );
    }

    #[test]
    fn an_empty_array_is_refused() {
        assert!(serde_json::from_str::<RecordKey>("[]").is_err());
    }

    /// The forgery a separator would allow.
    #[test]
    fn components_that_would_collide_under_a_join_stay_distinct() {
        let left = RecordKey::composite("a", ["bc"]);
        let right = RecordKey::composite("ab", ["c"]);
        assert_ne!(left, right);
        assert_ne!(left.to_netstring(), right.to_netstring());
        assert_ne!(
            serde_json::to_string(&left).expect("serialise"),
            serde_json::to_string(&right).expect("serialise")
        );
    }

    #[test]
    fn a_netstring_round_trips_every_component() {
        for key in [
            RecordKey::single(""),
            RecordKey::single("row-1"),
            RecordKey::composite("eu", ["owner:1", "row,1"]),
            RecordKey::composite("a", ["", "1:2,3", "\n"]),
        ] {
            let text = key.to_netstring();
            assert_eq!(RecordKey::from_netstring(&text).expect("parse"), key);
        }
    }

    #[test]
    fn a_malformed_netstring_is_refused() {
        for text in ["", "5:row-1", "x:row-1,", "9:short,"] {
            assert!(
                RecordKey::from_netstring(text).is_err(),
                "`{text}` must not parse"
            );
        }
    }

    #[test]
    fn display_renders_for_a_human_never_as_the_storage_form() {
        assert_eq!(RecordKey::single("row-1").to_string(), "row-1");
        assert_eq!(
            RecordKey::composite("eu", ["row-1"]).to_string(),
            "(eu, row-1)"
        );
        assert_ne!(
            RecordKey::single("row-1").to_string(),
            RecordKey::single("row-1").to_netstring(),
            "the human form and the storage codec must not be the same string"
        );
    }

    #[test]
    fn a_key_equals_a_bare_string_only_when_it_is_that_one_column() {
        assert_eq!(RecordKey::single("row-1"), "row-1");
        assert_eq!("row-1", RecordKey::single("row-1"));
        assert_ne!(RecordKey::composite("row", ["1"]), "row1");
        assert_ne!(RecordKey::composite("eu", ["row-1"]), "eu");
    }

    #[test]
    fn a_single_column_key_narrows_and_a_composite_one_refuses() {
        assert_eq!(RecordKey::single("row-1").as_single(), Some("row-1"));
        assert_eq!(
            RecordKey::composite("eu", ["row-1"]).as_single(),
            None,
            "narrowing must refuse rather than hand back the leading component"
        );
    }

    /// Keyset paging resumes on this order, so it must be componentwise.
    #[test]
    fn keys_order_componentwise() {
        assert!(RecordKey::composite("eu", ["a"]) < RecordKey::composite("eu", ["b"]));
        assert!(RecordKey::composite("eu", ["z"]) < RecordKey::composite("us", ["a"]));
        assert!(RecordKey::single("a") < RecordKey::composite("a", ["b"]));
    }
}
