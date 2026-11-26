//! The YSQL storage row for a relation tuple.

use fundamental_macros::Model;

use crate::backend::conversion::{caveat_to_columns, columns_to_tuple};
use crate::error::AuthzError;
use crate::tuple::RelationTuple;

/// One relation tuple as a YSQL row.
///
/// Keyed by the injective [`tuple_key`](RelationTupleRow::tuple_key), with the identity
/// fields stored too for the indexed forward and reverse reads.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(
    sql,
    table = "authz_relation_tuple",
    index(name = "authz_relation_tuple_forward_idx", columns = ["object_ns hash", "object_id hash", "relation hash"]),
    index(name = "authz_relation_tuple_reverse_idx", columns = ["subject_ns hash", "subject_id hash", "subject_relation hash", "object_ns asc", "relation asc"])
)]
pub(crate) struct RelationTupleRow {
    /// The injective identity key, the primary key.
    ///
    /// Hashed: every read by it is an exact match, so ordering would put writes on one tablet.
    #[model(id, shard = hash)]
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

impl RelationTupleRow {
    ///
    /// # Errors
    ///
    /// Returns an error if the caveat context fails to serialize.
    pub(crate) fn from_tuple(tuple: &RelationTuple) -> Result<Self, AuthzError> {
        let (caveat_name, caveat_context) = caveat_to_columns(&tuple.caveat)?;
        let subject_relation = tuple
            .subject
            .relation
            .as_ref()
            .map(|r| r.to_string())
            .unwrap_or_default();
        Ok(Self {
            tuple_key: tuple.key().as_str().to_string(),
            object_ns: tuple.object.namespace.to_string(),
            object_id: tuple.object.id.to_string(),
            relation: tuple.relation.to_string(),
            subject_ns: tuple.subject.object.namespace.to_string(),
            subject_id: tuple.subject.object.id.to_string(),
            subject_relation,
            caveat_name,
            caveat_context,
        })
    }

    ///
    /// # Errors
    ///
    /// Returns the specific [`AuthzError`] if any stored field is invalid.
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
