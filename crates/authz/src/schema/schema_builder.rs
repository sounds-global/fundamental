//! [`SchemaBuilder`]: explicit assembly of a schema from typed namespaces.

use std::collections::BTreeMap;

use super::{Namespace, NamespaceIr, NamespaceName, SchemaIr};
use crate::error::AuthzError;

/// Assembles a [`SchemaIr`] from typed `#[derive(Namespace)]` types and/or raw
/// [`NamespaceIr`] values, then validates the whole in [`build`](SchemaBuilder::build).
///
/// The first error is remembered and returned by `build`, so the chain stays fluent.
#[derive(Debug)]
#[must_use]
pub struct SchemaBuilder {
    namespaces: Result<BTreeMap<NamespaceName, NamespaceIr>, AuthzError>,
}

impl SchemaBuilder {
    /// Creates an empty builder.
    pub(super) fn new() -> Self {
        Self {
            namespaces: Ok(BTreeMap::new()),
        }
    }

    /// Adds a typed namespace `N`.
    pub fn namespace<N: Namespace>(self) -> Self {
        match NamespaceName::new(N::NAME) {
            Ok(_) => self.namespace_ir(N::ir()),
            Err(error) => self.fail(error),
        }
    }

    /// Adds a raw namespace definition.
    pub fn namespace_ir(mut self, ir: NamespaceIr) -> Self {
        if let Ok(namespaces) = &mut self.namespaces {
            namespaces.insert(ir.namespace.clone(), ir);
        }
        self
    }

    fn fail(mut self, error: AuthzError) -> Self {
        if self.namespaces.is_ok() {
            self.namespaces = Err(error);
        }
        self
    }

    /// Validates and compiles the assembled namespaces.
    ///
    /// # Errors
    ///
    /// Returns the first [`AuthzError`]: an invalid name recorded during assembly, or a dangling
    /// reference found by [`SchemaIr::compile`](super::SchemaIr::compile).
    pub fn build(self) -> Result<SchemaIr, AuthzError> {
        SchemaIr::compile(self.namespaces?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{NamespaceIr, RelatedType, RelationIr};

    fn ns(name: &str) -> NamespaceName {
        NamespaceName::new(name).expect("valid")
    }

    #[test]
    fn builds_from_namespace_ir() {
        let schema = SchemaBuilder::new()
            .namespace_ir(NamespaceIr::new(ns("user"), vec![]))
            .namespace_ir(NamespaceIr::new(
                ns("document"),
                vec![RelationIr::direct(
                    crate::schema::RelationName::new("owner").expect("valid"),
                    vec![RelatedType::leaf(ns("user"))],
                )],
            ))
            .build()
            .expect("valid schema");
        assert_eq!(schema.len(), 2);
    }

    #[test]
    fn surfaces_dangling_reference_at_build() {
        let err = SchemaBuilder::new()
            .namespace_ir(NamespaceIr::new(
                ns("document"),
                vec![RelationIr::direct(
                    crate::schema::RelationName::new("owner").expect("valid"),
                    vec![RelatedType::leaf(ns("user"))], // user namespace never added
                )],
            ))
            .build()
            .expect_err("missing user namespace");
        assert!(matches!(err, AuthzError::UndefinedNamespace { .. }));
    }
}
