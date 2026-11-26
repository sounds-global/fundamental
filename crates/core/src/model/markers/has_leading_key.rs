//! The [`HasLeadingKey`] query type state.

/// Type state: the query has pinned its leading key column to a single value.
///
/// Reached only through the builder's own transition, so a caller who supplied no key value cannot forge it.
#[derive(Debug, Clone, Copy, Default)]
pub struct HasLeadingKey;
