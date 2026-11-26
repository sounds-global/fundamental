//! [`SchemaLoader`]: compile a schema from serialized config text.

use std::collections::BTreeMap;

use serde::Deserialize;
use tracing::instrument;

use super::{NamespaceIr, NamespaceName, SchemaIr};
use crate::error::AuthzError;

#[derive(Debug, Deserialize)]
struct SchemaDoc {
    namespaces: Vec<NamespaceIr>,
}

/// Loads a schema from serialized config text.
///
/// Compiles through the same [`SchemaIr::compile`](super::SchemaIr::compile) validation as
/// `#[derive(Namespace)]`.
#[derive(Debug)]
pub struct SchemaLoader;

impl SchemaLoader {
    /// Compiles a schema from a JSON document of the form
    /// `{ "namespaces": [ { "namespace": "...", "relations": { ... } }, ... ] }`.
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::SchemaParse`] if the text is not valid JSON in the expected shape,
    /// or the validation error from `SchemaIr::compile` for a dangling reference.
    #[instrument(skip(json))]
    pub fn from_json(json: &str) -> Result<SchemaIr, AuthzError> {
        let doc: SchemaDoc =
            serde_json::from_str(json).map_err(|e| AuthzError::SchemaParse(e.to_string()))?;
        let namespaces: BTreeMap<NamespaceName, NamespaceIr> = doc
            .namespaces
            .into_iter()
            .map(|ns| (ns.namespace.clone(), ns))
            .collect();
        SchemaIr::compile(namespaces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_and_compiles_a_json_schema() {
        let json = r#"{
            "namespaces": [
                { "namespace": "user", "relations": {} },
                {
                    "namespace": "document",
                    "relations": {
                        "owner": {
                            "name": "owner",
                            "rewrite": "this",
                            "directly_related_types": [{ "namespace": "user" }]
                        }
                    }
                }
            ]
        }"#;
        let schema = SchemaLoader::from_json(json).expect("valid schema");
        assert_eq!(schema.len(), 2);
    }

    #[test]
    fn rejects_malformed_json() {
        let err = SchemaLoader::from_json("{ not json").expect_err("parse error");
        assert!(matches!(err, AuthzError::SchemaParse(_)));
    }

    #[test]
    fn rejects_unsafe_namespace_name_during_parse() {
        let json = r#"{ "namespaces": [ { "namespace": "bad name", "relations": {} } ] }"#;
        let err = SchemaLoader::from_json(json).expect_err("invalid name");
        assert!(matches!(err, AuthzError::SchemaParse(_)));
    }

    #[test]
    fn propagates_dangling_reference() {
        let json = r#"{
            "namespaces": [
                {
                    "namespace": "document",
                    "relations": {
                        "owner": {
                            "name": "owner",
                            "rewrite": "this",
                            "directly_related_types": [{ "namespace": "ghost" }]
                        }
                    }
                }
            ]
        }"#;
        let err = SchemaLoader::from_json(json).expect_err("dangling namespace");
        assert!(matches!(err, AuthzError::UndefinedNamespace { .. }));
    }
}
