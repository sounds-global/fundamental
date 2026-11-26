//! [`SchemaChanged`]: the event announcing that the loaded schema was replaced.

use serde::{Deserialize, Serialize};

use crate::schema::SchemaHash;
use crate::zookie::HybridTs;

/// Announces that the active schema was replaced (a hot reload), identified by its
/// new hash.
///
/// Lets a replica reload the schema and drop indexes built against the old one. Published on
/// `authz.events.schema.changed`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SchemaChanged {
    /// The hash of the schema now in force.
    pub schema_hash: SchemaHash,
    /// When the new schema took effect.
    pub effective_at: HybridTs,
}

impl SchemaChanged {
    /// Builds a schema-change event for `schema_hash` effective at `effective_at`.
    #[must_use]
    pub fn new(schema_hash: SchemaHash, effective_at: HybridTs) -> Self {
        Self {
            schema_hash,
            effective_at,
        }
    }
}
