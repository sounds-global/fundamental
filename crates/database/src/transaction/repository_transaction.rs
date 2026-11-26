//! Typed CRUD inside a transaction.
//!
//! The supertrait is [`TransactionScope`], so a handle lent to code that must not end the
//! transaction can still be a typed repository.

use super::TransactionScope;
use crate::DatabaseResult;
use fundamental_core::model::{BoundTo, Database};

/// Transaction-scoped typed CRUD for a model `M` on database `D`.
///
/// The counterpart of [`Repository`](crate::Repository); methods take `&mut self`.
pub trait RepositoryTransaction<M, D>: TransactionScope<D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// The ID type for the model.
    type Id: Send + Sync;

    /// Inserts a new model within the transaction.
    ///
    /// Returns the inserted model, with any generated fields populated.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the insert fails.
    fn insert(&mut self, model: &M) -> impl Future<Output = DatabaseResult<M>> + Send;

    /// Updates an existing model within the transaction.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the update fails.
    fn update(&mut self, model: &M) -> impl Future<Output = DatabaseResult<M>> + Send;

    /// Inserts or updates a model (upsert) within the transaction.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the upsert fails.
    fn upsert(&mut self, model: &M) -> impl Future<Output = DatabaseResult<M>> + Send;

    /// Inserts a model only if no row already holds its key, within the transaction.
    ///
    /// One statement conflicting on the model's own key columns: `Some(row)` when this call
    /// inserted it, `None` when a row already held the key, which is left untouched. On YSQL under
    /// `SERIALIZABLE`, concurrent callers inserting distinct keys this way deadlock one another, so
    /// a hot path there inserts plainly and reads a unique violation as "already there".
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the insert fails for any
    /// reason other than the key already being present.
    fn insert_if_absent(
        &mut self,
        model: &M,
    ) -> impl Future<Output = DatabaseResult<Option<M>>> + Send;
}
