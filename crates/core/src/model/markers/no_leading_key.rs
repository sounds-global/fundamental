//! The [`NoLeadingKey`] query type state.

/// Type state: the query has not pinned its leading key column.
///
/// Fine for a single-column key; for a composite key the query cannot run in it, see
/// [`LeadingKeyAddressed`](super::LeadingKeyAddressed).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoLeadingKey;
