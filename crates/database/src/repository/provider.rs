//! Repository provider trait.

use super::Repository;
use fundamental_core::model::{BoundTo, Database};

/// Trait for types that can provide repositories.
///
/// Implemented by connection pools and transaction handles.
pub trait RepositoryProvider<D>: Send + Sync
where
    D: Database,
{
    /// The repository type for a given model.
    type Repository<M>: Repository<M, D>
    where
        M: BoundTo<D> + Send + Sync;

    /// Returns a repository for the given model type.
    fn repository<M>(&self) -> Self::Repository<M>
    where
        M: BoundTo<D> + Send + Sync;
}
