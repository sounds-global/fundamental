//! [`ObjectRef`]: a reference to a specific object, a namespace plus an id.

use core::fmt;

use serde::{Deserialize, Serialize};

use super::CanonicalId;
use crate::schema::NamespaceName;

/// A reference to one object, such as `document:readme`; renders as `namespace:id`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ObjectRef {
    /// The object's namespace (its type).
    pub namespace: NamespaceName,
    /// The object's id within that namespace.
    pub id: CanonicalId,
}

impl ObjectRef {
    /// Builds an object reference.
    #[must_use]
    pub fn new(namespace: NamespaceName, id: CanonicalId) -> Self {
        Self { namespace, id }
    }
}

impl fmt::Display for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_canonical_form() {
        let obj = ObjectRef::new(
            NamespaceName::new("document").expect("valid"),
            CanonicalId::new("readme").expect("valid"),
        );
        assert_eq!(obj.to_string(), "document:readme");
    }
}
