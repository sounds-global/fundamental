//! Translation-companion search inside a transaction.

use super::RepositoryTransaction;
use crate::DatabaseResult;
use fundamental_core::model::{BoundTo, Database};

/// Transaction-scoped translation-companion search for a model `M` on database `D`.
///
/// The counterpart of [`RepositoryTranslation`](crate::RepositoryTranslation).
pub trait RepositoryTransactionTranslation<M, D>: RepositoryTransaction<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// The id type yielded by a translation search (YSQL: the model `Id`).
    type TranslationId;

    /// Finds ids whose translatable `field` matches `value` in `locale`, within
    /// the transaction.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the query fails.
    fn find_ids_by_translation(
        &mut self,
        field: &str,
        locale: &str,
        value: &str,
    ) -> impl Future<Output = DatabaseResult<Vec<Self::TranslationId>>> + Send;
}
