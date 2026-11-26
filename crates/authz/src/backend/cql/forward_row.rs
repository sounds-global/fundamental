//! The YCQL forward-table row, partitioned by object#relation.

use fundamental_macros::Model;

use crate::backend::conversion::{caveat_to_columns, columns_to_tuple};
use crate::error::AuthzError;
use crate::tuple::{RelationTuple, SubjectRef};

/// Clustering discriminator for a principal-leaf subject.
pub(crate) const SUBJECT_KIND_LEAF: &str = "l";
/// Clustering discriminator for a userset subject.
pub(crate) const SUBJECT_KIND_USERSET: &str = "u";

/// One relation tuple in the YCQL forward table, partitioned by `(object_ns, object_id,
/// relation)`.
///
/// The clustering key leads with `subject_kind`, so usersets are one equality
/// (`subject_kind = 'u'`); CQL cannot express `subject_relation != ''`.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(cql, table = "authz_relation_tuple")]
pub(crate) struct ForwardRow {
    #[model(partition_key)]
    pub object_ns: String,
    #[model(partition_key)]
    pub object_id: String,
    #[model(partition_key)]
    pub relation: String,
    /// `l` (leaf) or `u` (userset), the leading clustering column.
    #[model(clustering_key)]
    pub subject_kind: String,
    #[model(clustering_key)]
    pub subject_ns: String,
    #[model(clustering_key)]
    pub subject_id: String,
    /// Subject relation, empty for a leaf.
    #[model(clustering_key)]
    pub subject_relation: String,
    /// Caveat name, if the edge is conditional.
    pub caveat_name: Option<String>,
    /// Caveat context JSON, if any.
    pub caveat_context: Option<String>,
}

pub(crate) fn subject_kind(subject: &SubjectRef) -> &'static str {
    if subject.is_userset() {
        SUBJECT_KIND_USERSET
    } else {
        SUBJECT_KIND_LEAF
    }
}

impl ForwardRow {
    pub(crate) fn from_tuple(tuple: &RelationTuple) -> Result<Self, AuthzError> {
        let (caveat_name, caveat_context) = caveat_to_columns(&tuple.caveat)?;
        Ok(Self {
            object_ns: tuple.object.namespace.to_string(),
            object_id: tuple.object.id.to_string(),
            relation: tuple.relation.to_string(),
            subject_kind: subject_kind(&tuple.subject).to_string(),
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

    /// Reconstructs the logical tuple; `subject_kind` is derived, so not needed.
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
