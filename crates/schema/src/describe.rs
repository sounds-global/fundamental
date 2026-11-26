//! [`describe!`](crate::describe) — a type's schema when it has one, or [`SchemaNode::Any`] otherwise.

use core::marker::PhantomData;

use crate::schema::Schema;
use crate::schema_node::SchemaNode;

/// Describes a type structurally, whether or not it implements [`Schema`].
///
/// ```
/// use fundamental_schema::{describe, Schema, SchemaNode};
///
/// #[derive(Schema)]
/// struct Described { flag: bool }
///
/// struct Opaque;
///
/// assert!(matches!(describe!(Described), SchemaNode::Object { .. }));
/// assert_eq!(describe!(Opaque), SchemaNode::Any);
/// ```
#[macro_export]
macro_rules! describe {
    ($ty:ty) => {{
        #[expect(
            unused_imports,
            reason = "only one of the two probes answers for a given type"
        )]
        use $crate::describe::{ViaAny as _, ViaSchema as _};
        (&$crate::describe::Probe::<$ty>::new()).describe()
    }};
}

/// A carrier for `T` that the two `describe` impls dispatch on; public only for [`describe!`].
#[doc(hidden)]
pub struct Probe<T>(PhantomData<T>);

impl<T> Probe<T> {
    /// Constructs a probe.
    #[must_use]
    pub fn new() -> Self {
        Probe(PhantomData)
    }
}

impl<T> Default for Probe<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// The rich path when `T: Schema`, on `Probe<T>` so method resolution tries it first.
#[doc(hidden)]
pub trait ViaSchema {
    /// Returns the schema of the probed type.
    fn describe(&self) -> SchemaNode;
}

impl<T: Schema> ViaSchema for Probe<T> {
    fn describe(&self) -> SchemaNode {
        T::schema()
    }
}

/// The fallback for every `T`, on `&Probe<T>` so it is reached only after an extra autoref.
#[doc(hidden)]
pub trait ViaAny {
    /// Returns [`SchemaNode::Any`] for a type that does not implement [`Schema`].
    fn describe(&self) -> SchemaNode;
}

impl<T> ViaAny for &Probe<T> {
    fn describe(&self) -> SchemaNode {
        SchemaNode::Any
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Described;

    impl Schema for Described {
        fn schema() -> SchemaNode {
            SchemaNode::Bool
        }
    }

    struct Opaque;

    #[test]
    fn a_schema_type_takes_the_rich_path() {
        assert_eq!(describe!(Described), SchemaNode::Bool);
    }

    #[test]
    fn a_non_schema_type_falls_back_to_any() {
        assert_eq!(describe!(Opaque), SchemaNode::Any);
    }

    #[test]
    fn a_projected_type_resolves() {
        // The real callers describe an associated type; prove that resolves concretely.
        trait Carrier {
            type Payload;
        }
        struct Holder;
        impl Carrier for Holder {
            type Payload = Described;
        }
        assert_eq!(describe!(<Holder as Carrier>::Payload), SchemaNode::Bool);
    }
}
