//! The YCQL reverse-index row, partitioned by subject.

use fundamental_macros::Model;

use crate::backend::conversion::{caveat_to_columns, columns_to_tuple};
use crate::error::AuthzError;
use crate::tuple::RelationTuple;

/// One relation tuple in the YCQL reverse table, partitioned by subject.
///
/// Its own table, since YCQL has no cross-partition query; clustered by
/// `(object_ns, relation, object_id)` so list-objects narrows by prefix.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(cql, table = "authz_relation_tuple_reverse")]
pub(crate) struct ReverseRow {
    #[model(partition_key)]
    pub subject_ns: String,
    #[model(partition_key)]
    pub subject_id: String,
    /// Subject relation, empty for a leaf.
    #[model(partition_key)]
    pub subject_relation: String,
    /// Object namespace, the leading clustering column.
    #[model(clustering_key)]
    pub object_ns: String,
    #[model(clustering_key)]
    pub relation: String,
    #[model(clustering_key)]
    pub object_id: String,
    /// Caveat name, if the edge is conditional.
    pub caveat_name: Option<String>,
    /// Caveat context JSON, if any.
    pub caveat_context: Option<String>,
}

impl ReverseRow {
    pub(crate) fn from_tuple(tuple: &RelationTuple) -> Result<Self, AuthzError> {
        let (caveat_name, caveat_context) = caveat_to_columns(&tuple.caveat)?;
        Ok(Self {
            subject_ns: tuple.subject.object.namespace.to_string(),
            subject_id: tuple.subject.object.id.to_string(),
            subject_relation: tuple
                .subject
                .relation
                .as_ref()
                .map(|r| r.to_string())
                .unwrap_or_default(),
            object_ns: tuple.object.namespace.to_string(),
            relation: tuple.relation.to_string(),
            object_id: tuple.object.id.to_string(),
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
