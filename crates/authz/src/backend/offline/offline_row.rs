//! The offline (SQLite) storage row for a relation tuple.

use fundamental_macros::Model;

use crate::backend::conversion::{caveat_to_columns, columns_to_tuple};
use crate::error::AuthzError;
use crate::tuple::RelationTuple;

/// One relation tuple as an offline SQLite row, keyed by the injective
/// [`key`](OfflineRow::key) with the identity fields stored too for filtered reads.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline, offline_table = "authz_relation_tuple")]
pub(crate) struct OfflineRow {
    /// The injective identity key, the primary key.
    #[model(id)]
    pub tuple_key: String,
    /// The object's namespace.
    pub object_ns: String,
    /// The object's id.
    pub object_id: String,
    /// The relation.
    pub relation: String,
    /// The subject's namespace.
    pub subject_ns: String,
    /// The subject's id.
    pub subject_id: String,
    /// The subject's relation (empty for a principal leaf).
    pub subject_relation: String,
    /// The caveat name, if the edge is conditional.
    pub caveat_name: Option<String>,
    /// The caveat context as JSON, if any.
    pub caveat_context: Option<String>,
}

impl OfflineRow {
    pub(crate) fn from_tuple(tuple: &RelationTuple) -> Result<Self, AuthzError> {
        let (caveat_name, caveat_context) = caveat_to_columns(&tuple.caveat)?;
        Ok(Self {
            tuple_key: tuple.key().as_str().to_string(),
            object_ns: tuple.object.namespace.to_string(),
            object_id: tuple.object.id.to_string(),
            relation: tuple.relation.to_string(),
            subject_ns: tuple.subject.object.namespace.to_string(),
            subject_id: tuple.subject.object.id.to_string(),
            subject_relation: tuple
                .subject
                .relation
                .as_ref()
                .map(|r| r.to_string())
                .unwrap_or_default(),
            caveat_name,
            caveat_context,
        })
    }

    pub(crate) fn into_tuple(self) -> Result<RelationTuple, AuthzError> {
        columns_to_tuple(
            self.object_ns,
            self.object_id,
            self.relation,
            self.subject_ns,
            self.subject_id,
            self.subject_relation,
            self.caveat_name,
            self.caveat_context,
        )
    }
}
