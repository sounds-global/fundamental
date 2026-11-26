//! The tuple-store column names, shared by every backend.

/// The injective identity key, the single-column primary key on the relational and offline
/// backends.
#[cfg(feature = "sql")]
pub(crate) const TUPLE_KEY: &str = "tuple_key";
pub(crate) const OBJECT_NS: &str = "object_ns";
/// Object id column; YCQL carries it in its partition key and never names it.
#[cfg(any(feature = "sql", feature = "offline"))]
pub(crate) const OBJECT_ID: &str = "object_id";
pub(crate) const RELATION: &str = "relation";
pub(crate) const SUBJECT_NS: &str = "subject_ns";
pub(crate) const SUBJECT_ID: &str = "subject_id";
/// Subject relation column, empty for a principal leaf.
pub(crate) const SUBJECT_RELATION: &str = "subject_relation";
/// YCQL-only clustering discriminator (`l` leaf / `u` userset).
#[cfg(feature = "cql")]
pub(crate) const SUBJECT_KIND: &str = "subject_kind";
