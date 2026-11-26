use fundamental_core::Model;
use fundamental_core::model::{BoundTo, SqlDb, SqlModel};
use fundamental_db::migration::ModelSchema;
use fundamental_db::{QueryExecutor, Repository};

use crate::{FromRow, SqlPool, SqlRepository, ToRow};

/// Where code finds each model's repository: a pool in production, a memory database in a test.
///
/// ```rust,ignore
/// async fn published<R: Repositories>(rows: &R, organisation: Uuid) -> DatabaseResult<Vec<Project>> {
///     let projects = rows.repository::<Project>();
///     projects.execute(&projects.query().pin_key(organisation)).await
/// }
/// ```
pub trait Repositories: Clone + Send + Sync + 'static {
    /// The repository of `M`'s rows.
    type Repository<M>: Repository<M, SqlDb> + QueryExecutor<M, SqlDb> + Clone + Send + Sync
    where
        M: Stored;

    /// The repository of `M`'s rows.
    fn repository<M: Stored>(&self) -> Self::Repository<M>;
}

/// A SQL model that reads and binds its columns and names its table, so a [`Repositories`] can hold it.
pub trait Stored:
    BoundTo<SqlDb>
    + SqlModel<Id: Send + Sync>
    + Model
    + ModelSchema<SqlDb>
    + FromRow
    + ToRow
    + Clone
    + Send
    + Sync
    + 'static
{
}

impl<M> Stored for M where
    M: BoundTo<SqlDb>
        + SqlModel<Id: Send + Sync>
        + Model
        + ModelSchema<SqlDb>
        + FromRow
        + ToRow
        + Clone
        + Send
        + Sync
        + 'static
{
}

impl Repositories for SqlPool {
    type Repository<M>
        = SqlRepository<M>
    where
        M: Stored;

    fn repository<M: Stored>(&self) -> SqlRepository<M> {
        Self::repository(self)
    }
}
