//! Cross-backend search of a translatable model's companion table.

use super::{DatabaseResult, Repository};
use fundamental_core::model::{BoundTo, Database};

/// Search a model's translation companion table by a translated value, returning
/// the matching ids.
pub trait RepositoryTranslation<M, D>: Repository<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// The id type yielded by a translation search (YSQL: the model `Id`;
    /// Offline: the key column's `Value`; YCQL: the key tuple, `ToRow::KeyValues`).
    type TranslationId;

    /// Finds ids whose translatable `field` matches `value` in `locale`.
    fn find_ids_by_translation(
        &self,
        field: &str,
        locale: &str,
        value: &str,
    ) -> impl std::future::Future<Output = DatabaseResult<Vec<Self::TranslationId>>> + Send;
}
