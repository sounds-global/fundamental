//! [`SchemaNode`] — a typed JSON-Schema-2020-12 model of a type's shape.

use serde::Serialize;
use serde::ser::{SerializeMap, SerializeSeq, Serializer};

/// A structural description of a type, shaped as JSON Schema 2020-12.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SchemaNode {
    /// Accepts any value: the empty schema `{}`.
    Any,
    /// A boolean.
    Bool,
    /// An integer, optionally bounded; a bound that does not fit an `i64` is left open.
    Integer {
        /// Inclusive lower bound, when it fits an `i64`.
        minimum: Option<i64>,
        /// Inclusive upper bound, when it fits an `i64`.
        maximum: Option<i64>,
    },
    /// A floating-point number.
    Number,
    /// A string, optionally with a `format` and a `pattern`.
    String {
        /// A JSON Schema `format` hint, such as `"uuid"` or `"email"`.
        format: Option<&'static str>,
        /// A regular-expression the string must match.
        pattern: Option<String>,
    },
    /// A homogeneous list of `items`.
    Array(Box<SchemaNode>),
    /// A map with string keys and `additionalProperties` of the inner shape.
    Map(Box<SchemaNode>),
    /// An object with named `properties`, a subset of which are `required`.
    Object {
        /// The object's fields, in declaration order.
        properties: Vec<(String, SchemaNode)>,
        /// The names of the fields that must be present.
        required: Vec<String>,
    },
    /// A closed set of string values (a fieldless enum).
    Enum {
        /// The permitted string values, in declaration order.
        values: Vec<String>,
    },
    /// The inner shape, or `null`.
    Nullable(Box<SchemaNode>),
}

impl SchemaNode {
    /// The schema that accepts anything (`{}`).
    #[must_use]
    pub fn any() -> Self {
        SchemaNode::Any
    }

    /// Wraps this node so it also accepts `null`, without double-wrapping.
    #[must_use]
    pub fn nullable(self) -> Self {
        match self {
            SchemaNode::Nullable(_) => self,
            other => SchemaNode::Nullable(Box::new(other)),
        }
    }
}

impl Serialize for SchemaNode {
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            SchemaNode::Any => serializer.serialize_map(Some(0))?.end(),
            SchemaNode::Bool => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("type", "boolean")?;
                map.end()
            }
            SchemaNode::Integer { minimum, maximum } => {
                let count = 1 + usize::from(minimum.is_some()) + usize::from(maximum.is_some());
                let mut map = serializer.serialize_map(Some(count))?;
                map.serialize_entry("type", "integer")?;
                if let Some(minimum) = minimum {
                    map.serialize_entry("minimum", minimum)?;
                }
                if let Some(maximum) = maximum {
                    map.serialize_entry("maximum", maximum)?;
                }
                map.end()
            }
            SchemaNode::Number => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("type", "number")?;
                map.end()
            }
            SchemaNode::String { format, pattern } => {
                let count = 1 + usize::from(format.is_some()) + usize::from(pattern.is_some());
                let mut map = serializer.serialize_map(Some(count))?;
                map.serialize_entry("type", "string")?;
                if let Some(format) = format {
                    map.serialize_entry("format", format)?;
                }
                if let Some(pattern) = pattern {
                    map.serialize_entry("pattern", pattern)?;
                }
                map.end()
            }
            SchemaNode::Array(items) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "array")?;
                map.serialize_entry("items", items)?;
                map.end()
            }
            SchemaNode::Map(values) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "object")?;
                map.serialize_entry("additionalProperties", values)?;
                map.end()
            }
            SchemaNode::Object {
                properties,
                required,
            } => {
                let count = 2 + usize::from(!required.is_empty());
                let mut map = serializer.serialize_map(Some(count))?;
                map.serialize_entry("type", "object")?;
                map.serialize_entry("properties", &Properties(properties))?;
                if !required.is_empty() {
                    map.serialize_entry("required", required)?;
                }
                map.end()
            }
            SchemaNode::Enum { values } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "string")?;
                map.serialize_entry("enum", values)?;
                map.end()
            }
            SchemaNode::Nullable(inner) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("anyOf", &AnyOfNull(inner))?;
                map.end()
            }
        }
    }
}

struct Properties<'a>(&'a [(String, SchemaNode)]);

impl Serialize for Properties<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, node) in self.0 {
            map.serialize_entry(name, node)?;
        }
        map.end()
    }
}

struct AnyOfNull<'a>(&'a SchemaNode);

impl Serialize for AnyOfNull<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(2))?;
        seq.serialize_element(self.0)?;
        seq.serialize_element(&NullType)?;
        seq.end()
    }
}

struct NullType;

impl Serialize for NullType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("type", "null")?;
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(node: &SchemaNode) -> serde_json::Value {
        serde_json::to_value(node).expect("a schema node serializes")
    }

    #[test]
    fn any_serializes_to_the_empty_schema() {
        assert_eq!(json(&SchemaNode::Any), serde_json::json!({}));
    }

    #[test]
    fn bounded_integer_carries_its_range() {
        let node = SchemaNode::Integer {
            minimum: Some(0),
            maximum: Some(255),
        };
        assert_eq!(
            json(&node),
            serde_json::json!({ "type": "integer", "minimum": 0, "maximum": 255 })
        );
    }

    #[test]
    fn string_omits_absent_format_and_pattern() {
        let node = SchemaNode::String {
            format: None,
            pattern: None,
        };
        assert_eq!(json(&node), serde_json::json!({ "type": "string" }));
    }

    #[test]
    fn object_lists_required_and_preserves_order() {
        let node = SchemaNode::Object {
            properties: vec![
                (
                    "name".into(),
                    SchemaNode::String {
                        format: None,
                        pattern: None,
                    },
                ),
                (
                    "age".into(),
                    SchemaNode::Integer {
                        minimum: Some(0),
                        maximum: Some(255),
                    },
                ),
            ],
            required: vec!["name".into()],
        };
        assert_eq!(
            json(&node),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "age": { "type": "integer", "minimum": 0, "maximum": 255 }
                },
                "required": ["name"]
            })
        );
    }

    #[test]
    fn nullable_wraps_in_any_of_null() {
        let node = SchemaNode::Bool.nullable();
        assert_eq!(
            json(&node),
            serde_json::json!({ "anyOf": [{ "type": "boolean" }, { "type": "null" }] })
        );
    }

    #[test]
    fn nullable_does_not_double_wrap() {
        let once = SchemaNode::Bool.nullable();
        assert_eq!(once.clone().nullable(), once);
    }

    #[test]
    fn enum_serializes_as_a_string_with_values() {
        let node = SchemaNode::Enum {
            values: vec!["red".into(), "green".into()],
        };
        assert_eq!(
            json(&node),
            serde_json::json!({ "type": "string", "enum": ["red", "green"] })
        );
    }

    #[test]
    fn map_uses_additional_properties() {
        let node = SchemaNode::Map(Box::new(SchemaNode::Number));
        assert_eq!(
            json(&node),
            serde_json::json!({ "type": "object", "additionalProperties": { "type": "number" } })
        );
    }
}
