//! [`Schema`] — the contract that describes a type's structural shape.

use crate::schema_node::SchemaNode;

/// A type whose structural shape is known and can be described as a [`SchemaNode`].
pub trait Schema {
    /// Returns this type's structural description.
    fn schema() -> SchemaNode;
}
