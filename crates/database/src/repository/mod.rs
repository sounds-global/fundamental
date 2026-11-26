//! Repository pattern traits, generic over the database type. `pool` and `User` come from a
//! backend crate, so this sketch is not compiled:
//!
//! ```ignore
//! // Get a repository for the User model
//! let users = pool.repository::<User>();
//!
//! // CRUD operations
//! let user = users.find(&user_id).await?;
//! users.insert(&new_user).await?;
//! users.update(&updated_user).await?;
//! users.delete(&user_id).await?;
//! ```

use super::{DatabaseResult, QueryBuilder};
use fundamental_core::model::{BoundTo, Database};

mod aggregate;
mod batch;
mod ext;
mod keyed_batch;
mod provider;
mod soft_delete;
mod translation;

pub use aggregate::RepositoryAggregate;
pub use batch::RepositoryBatch;
pub use ext::RepositoryExt;
pub use keyed_batch::KeyedRepositoryBatch;
pub use provider::RepositoryProvider;
pub use soft_delete::RepositorySoftDelete;
pub use translation::RepositoryTranslation;

/// Core repository trait for database operations.
///
/// The by-id operations live in [`KeyedRepository`]: they need a bindable id, and a
/// composite-key model's `Id` is a marker that is deliberately not, so it keeps this half.
pub trait Repository<M, D>: Send + Sync
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// The ID type for the model.
    type Id: Send + Sync;

    /// Inserts a new model.
    ///
    /// Returns the inserted model, with any generated fields populated.
    fn insert(&self, model: &M) -> impl Future<Output = DatabaseResult<M>> + Send;

    /// Updates an existing model.
    fn update(&self, model: &M) -> impl Future<Output = DatabaseResult<M>> + Send;

    /// Inserts or updates a model (upsert).
    ///
    /// On YSQL `ON CONFLICT UPDATE`; on YCQL a plain `INSERT`, which is an upsert already.
    fn upsert(&self, model: &M) -> impl Future<Output = DatabaseResult<M>> + Send;

    /// Returns a query builder for complex queries.
    ///
    /// How a composite-key model is read: every key column named in a filter.
    fn query(&self) -> QueryBuilder<M, D>;
}

/// The operations that address a row **by its id**, for models whose id can do that.
///
/// Not implemented for a composite-key model, where `find` on part of the key would scan and
/// return whichever tenant matched first. Read such a row through [`Repository::query`].
pub trait KeyedRepository<M, D>: Repository<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Finds a model by its ID.
    ///
    /// Returns `None` if not found.
    fn find(&self, id: &Self::Id) -> impl Future<Output = DatabaseResult<Option<M>>> + Send;

    /// Finds a model by its ID, returning an error if not found.
    fn find_required(&self, id: &Self::Id) -> impl Future<Output = DatabaseResult<M>> + Send;

    /// Checks if a model with the given ID exists.
    fn exists(&self, id: &Self::Id) -> impl Future<Output = DatabaseResult<bool>> + Send;

    /// Deletes a model by its ID.
    ///
    /// Returns `true` if a model was deleted, `false` if not found.
    fn delete(&self, id: &Self::Id) -> impl Future<Output = DatabaseResult<bool>> + Send;
}
