//! [`AuthzEvents`]: the message set of authorization change events.

use fundamental_nats::MessageSet;

use super::{SchemaChanged, TupleChanged};

/// The one-way events the authorization system publishes, gathered behind a single
/// set so a replica subscribes them all through one handler.
///
/// A write or a schema reload fans out here so replicas keep their caches honest; the subjects
/// derive from the names ([`TupleChanged`] on `authz.events.tuple.changed`).
#[derive(Debug, Clone, MessageSet)]
pub enum AuthzEvents {
    /// Relationship tuples changed — carries the watermark and touched objects.
    TupleChanged(TupleChanged),
    /// The active schema was replaced — carries the new schema hash.
    SchemaChanged(SchemaChanged),
}

#[cfg(test)]
mod tests {
    use fundamental_nats::MessageSet as _;

    use super::*;

    #[test]
    fn subjects_derive_from_the_enum_and_variant_names() {
        assert!(
            AuthzEvents::SUBJECTS.contains(&"authz.events.tuple.changed"),
            "subjects were {:?}",
            AuthzEvents::SUBJECTS
        );
        assert!(AuthzEvents::SUBJECTS.contains(&"authz.events.schema.changed"));
    }
}
