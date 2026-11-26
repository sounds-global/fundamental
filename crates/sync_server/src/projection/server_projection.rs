use crate::projection::ProjectionRegistry;

/// A projection's server-side registration, emitted by `#[projection]`.
pub trait ServerProjection {
    /// Registers this projection's recompute handlers, one per source table.
    fn register(registry: ProjectionRegistry) -> ProjectionRegistry;
}
