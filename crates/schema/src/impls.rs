//! [`Schema`] implementations for standard-library and framework wire primitives.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use fundamental_core::secrets::Secret;
use uuid::Uuid;

use crate::schema::Schema;
use crate::schema_node::SchemaNode;

macro_rules! integer_schema {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Schema for $ty {
                fn schema() -> SchemaNode {
                    SchemaNode::Integer {
                        minimum: i64::try_from(<$ty>::MIN).ok(),
                        maximum: i64::try_from(<$ty>::MAX).ok(),
                    }
                }
            }
        )*
    };
}

integer_schema!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl Schema for bool {
    fn schema() -> SchemaNode {
        SchemaNode::Bool
    }
}

impl Schema for f32 {
    fn schema() -> SchemaNode {
        SchemaNode::Number
    }
}

impl Schema for f64 {
    fn schema() -> SchemaNode {
        SchemaNode::Number
    }
}

macro_rules! string_schema {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Schema for $ty {
                fn schema() -> SchemaNode {
                    SchemaNode::String { format: None, pattern: None }
                }
            }
        )*
    };
}

string_schema!(String, str, char);

/// A day, written as JSON Schema's `date`: `2026-01-05`.
impl Schema for chrono::NaiveDate {
    fn schema() -> SchemaNode {
        SchemaNode::String {
            format: Some("date"),
            pattern: None,
        }
    }
}

/// A moment, written as JSON Schema's `date-time` (RFC 3339): `2026-01-05T10:00:00Z`.
impl Schema for chrono::DateTime<chrono::Utc> {
    fn schema() -> SchemaNode {
        SchemaNode::String {
            format: Some("date-time"),
            pattern: None,
        }
    }
}

/// An exact amount of money, written with its denomination's code: `EUR 2200.00`.
impl Schema for fundamental_core::money::Money {
    fn schema() -> SchemaNode {
        SchemaNode::String {
            format: Some("money"),
            pattern: None,
        }
    }
}

impl Schema for Uuid {
    fn schema() -> SchemaNode {
        SchemaNode::String {
            format: Some("uuid"),
            pattern: None,
        }
    }
}

impl<T: Schema + ?Sized> Schema for &T {
    fn schema() -> SchemaNode {
        T::schema()
    }
}

impl<T: Schema + ?Sized> Schema for Box<T> {
    fn schema() -> SchemaNode {
        T::schema()
    }
}

impl<T: Schema + ToOwned + ?Sized> Schema for Cow<'_, T> {
    fn schema() -> SchemaNode {
        T::schema()
    }
}

impl<T: Schema> Schema for Option<T> {
    fn schema() -> SchemaNode {
        T::schema().nullable()
    }
}

impl<T: Schema> Schema for Vec<T> {
    fn schema() -> SchemaNode {
        SchemaNode::Array(Box::new(T::schema()))
    }
}

/// A value not yet admitted describes as the string it arrives as.
impl<T> Schema for fundamental_core::model::Unadmitted<T> {
    fn schema() -> SchemaNode {
        SchemaNode::String {
            format: None,
            pattern: None,
        }
    }
}

/// A bounded list describes as an array; the bound is enforced by `BoundedVec`'s deserialiser.
impl<T: Schema, const MAX: usize> Schema for fundamental_core::bounded::BoundedVec<T, MAX> {
    fn schema() -> SchemaNode {
        SchemaNode::Array(Box::new(T::schema()))
    }
}

impl<T: Schema> Schema for [T] {
    fn schema() -> SchemaNode {
        SchemaNode::Array(Box::new(T::schema()))
    }
}

impl<T: Schema, const N: usize> Schema for [T; N] {
    fn schema() -> SchemaNode {
        SchemaNode::Array(Box::new(T::schema()))
    }
}

impl<K, V: Schema, S> Schema for HashMap<K, V, S> {
    fn schema() -> SchemaNode {
        SchemaNode::Map(Box::new(V::schema()))
    }
}

impl<K, V: Schema> Schema for BTreeMap<K, V> {
    fn schema() -> SchemaNode {
        SchemaNode::Map(Box::new(V::schema()))
    }
}

/// A stored secret describes as its inner shape, never carrying the value.
impl<T: Schema> Schema for Secret<T> {
    fn schema() -> SchemaNode {
        T::schema()
    }
}

/// A free-form JSON value describes as [`SchemaNode::Any`].
impl Schema for serde_json::Value {
    fn schema() -> SchemaNode {
        SchemaNode::Any
    }
}

impl Schema for serde_json::Map<String, serde_json::Value> {
    fn schema() -> SchemaNode {
        SchemaNode::Map(Box::new(SchemaNode::Any))
    }
}

/// A text kept in several languages describes as it is sent.
impl<T: Schema> Schema for fundamental_core::i18n::Localized<T> {
    fn schema() -> SchemaNode {
        let stamp = || SchemaNode::String {
            format: None,
            pattern: None,
        };
        SchemaNode::Object {
            properties: vec![
                ("source".to_owned(), T::schema()),
                (
                    "translations".to_owned(),
                    SchemaNode::Map(Box::new(T::schema())),
                ),
                (
                    "checks".to_owned(),
                    SchemaNode::Map(Box::new(SchemaNode::Object {
                        properties: vec![
                            ("source_hash".to_owned(), stamp()),
                            ("translation_hash".to_owned(), stamp()),
                            ("checked".to_owned(), SchemaNode::Any.nullable()),
                        ],
                        required: Vec::new(),
                    })),
                ),
            ],
            required: vec!["source".to_owned()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_in_several_languages_describes_as_it_is_sent() {
        let text: fundamental_core::i18n::Localized<String> =
            fundamental_core::i18n::Localized::new("Open").with("nl", "Open");
        let sent = serde_json::to_value(&text).expect("sent");
        let SchemaNode::Object {
            properties,
            required,
        } = <fundamental_core::i18n::Localized<String> as Schema>::schema()
        else {
            panic!("an object");
        };
        assert_eq!(required, ["source"]);
        for key in sent.as_object().expect("an object").keys() {
            assert!(properties.iter().any(|(name, _)| name == key), "{key}");
        }
    }

    use fundamental_core::secrets::SecretString;

    #[test]
    fn unsigned_byte_is_bounded_zero_to_255() {
        assert_eq!(
            u8::schema(),
            SchemaNode::Integer {
                minimum: Some(0),
                maximum: Some(255)
            }
        );
    }

    #[test]
    fn u64_leaves_its_maximum_open() {
        assert_eq!(
            u64::schema(),
            SchemaNode::Integer {
                minimum: Some(0),
                maximum: None
            }
        );
    }

    #[test]
    fn option_is_nullable_inner() {
        assert_eq!(
            Option::<bool>::schema(),
            SchemaNode::Nullable(Box::new(SchemaNode::Bool))
        );
    }

    #[test]
    fn vec_is_an_array_of_inner() {
        assert_eq!(
            Vec::<String>::schema(),
            SchemaNode::Array(Box::new(SchemaNode::String {
                format: None,
                pattern: None
            }))
        );
    }

    #[test]
    fn map_describes_value_shape() {
        assert_eq!(
            HashMap::<String, u8>::schema(),
            SchemaNode::Map(Box::new(SchemaNode::Integer {
                minimum: Some(0),
                maximum: Some(255)
            }))
        );
    }

    #[test]
    fn json_value_accepts_anything() {
        assert_eq!(serde_json::Value::schema(), SchemaNode::Any);
    }

    #[test]
    fn json_object_is_a_map_of_any() {
        assert_eq!(
            <serde_json::Map<String, serde_json::Value>>::schema(),
            SchemaNode::Map(Box::new(SchemaNode::Any))
        );
    }

    #[test]
    fn uuid_is_a_formatted_string() {
        assert_eq!(
            Uuid::schema(),
            SchemaNode::String {
                format: Some("uuid"),
                pattern: None
            }
        );
    }

    #[test]
    fn secret_string_describes_as_a_string() {
        assert_eq!(
            SecretString::schema(),
            SchemaNode::String {
                format: None,
                pattern: None
            }
        );
    }

    #[test]
    fn primitives_are_reachable_through_describe() {
        assert_eq!(crate::describe!(bool), SchemaNode::Bool);
    }
}
