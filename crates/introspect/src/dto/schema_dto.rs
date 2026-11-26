use serde::{Deserialize, Serialize};

/// One named field of an [`SchemaDto::Object`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaField {
    /// The field's name.
    pub name: String,
    /// The field's shape.
    pub schema: SchemaDto,
}

/// A structural mirror of [`SchemaNode`](fundamental_schema::SchemaNode), round-trippable through serde.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaDto {
    /// Accepts any value.
    Any,
    /// A boolean.
    Bool,
    /// An integer, optionally bounded.
    Integer {
        /// Inclusive lower bound, when it fits an `i64`.
        minimum: Option<i64>,
        /// Inclusive upper bound, when it fits an `i64`.
        maximum: Option<i64>,
    },
    /// A floating-point number.
    Number,
    /// A string, optionally carrying a `format` and a regex `pattern`.
    Text {
        /// A JSON Schema `format` hint (e.g. `"uuid"`).
        format: Option<String>,
        /// A regular expression the string must match.
        pattern: Option<String>,
    },
    /// A homogeneous list of `items`.
    Array {
        /// The element shape.
        items: Box<SchemaDto>,
    },
    /// A string-keyed map with values of the inner shape.
    Map {
        /// The value shape.
        values: Box<SchemaDto>,
    },
    /// An object with named `fields`, a subset of which are `required`.
    Object {
        /// The object's fields, in declaration order.
        fields: Vec<SchemaField>,
        /// The names of the fields that must be present.
        required: Vec<String>,
    },
    /// A closed set of string values.
    Enum {
        /// The permitted values, in declaration order.
        values: Vec<String>,
    },
    /// The inner shape, or `null`.
    Nullable {
        /// The wrapped shape.
        inner: Box<SchemaDto>,
    },
}

impl SchemaDto {
    /// A concise type label (`"string"`, `"User[]"`, `"active | archived"`, `"boolean?"`).
    #[must_use]
    pub fn type_label(&self) -> String {
        match self {
            SchemaDto::Any => "any".to_string(),
            SchemaDto::Bool => "boolean".to_string(),
            SchemaDto::Integer { .. } => "integer".to_string(),
            SchemaDto::Number => "number".to_string(),
            SchemaDto::Text { format, .. } => match format {
                Some(format) => format!("string<{format}>"),
                None => "string".to_string(),
            },
            SchemaDto::Array { items } => format!("{}[]", items.type_label()),
            SchemaDto::Map { values } => format!("map<string, {}>", values.type_label()),
            SchemaDto::Object { .. } => "object".to_string(),
            SchemaDto::Enum { values } => values.join(" | "),
            SchemaDto::Nullable { inner } => format!("{}?", inner.type_label()),
        }
    }

    /// The object's fields and required set, through a `Nullable`; `None` for anything else.
    #[must_use]
    pub fn as_object(&self) -> Option<(&[SchemaField], &[String])> {
        match self {
            SchemaDto::Object { fields, required } => Some((fields, required)),
            SchemaDto::Nullable { inner } => inner.as_object(),
            _ => None,
        }
    }
}

#[cfg(feature = "ssr")]
impl From<&fundamental_schema::SchemaNode> for SchemaDto {
    fn from(node: &fundamental_schema::SchemaNode) -> Self {
        use fundamental_schema::SchemaNode;
        match node {
            SchemaNode::Any => SchemaDto::Any,
            SchemaNode::Bool => SchemaDto::Bool,
            SchemaNode::Integer { minimum, maximum } => SchemaDto::Integer {
                minimum: *minimum,
                maximum: *maximum,
            },
            SchemaNode::Number => SchemaDto::Number,
            SchemaNode::String { format, pattern } => SchemaDto::Text {
                format: format.map(str::to_owned),
                pattern: pattern.clone(),
            },
            SchemaNode::Array(items) => SchemaDto::Array {
                items: Box::new(SchemaDto::from(items.as_ref())),
            },
            SchemaNode::Map(values) => SchemaDto::Map {
                values: Box::new(SchemaDto::from(values.as_ref())),
            },
            SchemaNode::Object {
                properties,
                required,
            } => SchemaDto::Object {
                fields: properties
                    .iter()
                    .map(|(name, schema)| SchemaField {
                        name: name.clone(),
                        schema: SchemaDto::from(schema),
                    })
                    .collect(),
                required: required.clone(),
            },
            SchemaNode::Enum { values } => SchemaDto::Enum {
                values: values.clone(),
            },
            SchemaNode::Nullable(inner) => SchemaDto::Nullable {
                inner: Box::new(SchemaDto::from(inner.as_ref())),
            },
            // `SchemaNode` is `#[non_exhaustive]`: a new variant renders as `Any`.
            _ => SchemaDto::Any,
        }
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use fundamental_schema::SchemaNode;

    #[test]
    fn every_known_variant_maps_to_its_mirror() {
        assert_eq!(SchemaDto::from(&SchemaNode::Any), SchemaDto::Any);
        assert_eq!(SchemaDto::from(&SchemaNode::Bool), SchemaDto::Bool);
        assert_eq!(SchemaDto::from(&SchemaNode::Number), SchemaDto::Number);
        assert_eq!(
            SchemaDto::from(&SchemaNode::Integer {
                minimum: Some(0),
                maximum: Some(255)
            }),
            SchemaDto::Integer {
                minimum: Some(0),
                maximum: Some(255)
            }
        );
        assert_eq!(
            SchemaDto::from(&SchemaNode::String {
                format: Some("uuid"),
                pattern: None
            }),
            SchemaDto::Text {
                format: Some("uuid".into()),
                pattern: None
            }
        );
        assert_eq!(
            SchemaDto::from(&SchemaNode::Enum {
                values: vec!["a".into(), "b".into()]
            }),
            SchemaDto::Enum {
                values: vec!["a".into(), "b".into()]
            }
        );
    }

    #[test]
    fn nested_object_arrays_and_nullable_recurse() {
        let node = SchemaNode::Object {
            properties: vec![
                (
                    "tags".into(),
                    SchemaNode::Array(Box::new(SchemaNode::String {
                        format: None,
                        pattern: None,
                    })),
                ),
                (
                    "note".into(),
                    SchemaNode::Nullable(Box::new(SchemaNode::Bool)),
                ),
            ],
            required: vec!["tags".into()],
        };
        let SchemaDto::Object { fields, required } = SchemaDto::from(&node) else {
            panic!("expected object");
        };
        assert_eq!(required, vec!["tags".to_string()]);
        assert_eq!(fields[0].name, "tags");
        assert!(matches!(fields[0].schema, SchemaDto::Array { .. }));
        assert!(matches!(fields[1].schema, SchemaDto::Nullable { .. }));
    }

    #[test]
    fn round_trips_through_serde() {
        let dto = SchemaDto::Object {
            fields: vec![SchemaField {
                name: "id".into(),
                schema: SchemaDto::Text {
                    format: Some("uuid".into()),
                    pattern: None,
                },
            }],
            required: vec!["id".into()],
        };
        let json = serde_json::to_string(&dto).expect("serialize");
        let back: SchemaDto = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(dto, back);
    }
}
