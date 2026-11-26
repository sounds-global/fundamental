use std::marker::PhantomData;

use chrono::Utc;
use fundamental_core::model::{BoundTo, LeadingKeyAddressed, Model, Satisfied, SqlDb, SqlModel};
use fundamental_db::migration::ModelSchema;
use fundamental_db::{
    DatabaseError, DatabaseResult, FilterValue, KeyedRepository, QueryBuilder, QueryExecutor,
    Repository, RepositorySoftDelete,
};
use fundamental_sql::ToRow;
use fundamental_sql::sqlx::{Encode, Postgres, Type};

use crate::MemoryDatabase;
use crate::cells::{cell_of, column};
use crate::compare::same;
use crate::memory_database::Row;
use crate::reading::{Deleted, chosen, key, ordered, paged, same_key, selected};
use crate::unique::{refuse_clash, unique_indexes};

/// One model's rows in a [`MemoryDatabase`].
pub struct MemoryRepository<M> {
    database: MemoryDatabase,
    _model: PhantomData<fn() -> M>,
}

impl<M> MemoryRepository<M> {
    pub(crate) const fn new(database: MemoryDatabase) -> Self {
        Self {
            database,
            _model: PhantomData,
        }
    }
}

impl<M> Clone for MemoryRepository<M> {
    fn clone(&self) -> Self {
        Self::new(self.database.clone())
    }
}

impl<M> std::fmt::Debug for MemoryRepository<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryRepository")
            .field("model", &std::any::type_name::<M>())
            .finish()
    }
}

/// A SQL model whose rows bind their columns and which names its table, so it can be held in memory.
pub trait Held:
    BoundTo<SqlDb> + SqlModel + Model + ModelSchema<SqlDb> + ToRow + Clone + Send + Sync + 'static
{
}

impl<M> Held for M where
    M: BoundTo<SqlDb>
        + SqlModel
        + Model
        + ModelSchema<SqlDb>
        + ToRow
        + Clone
        + Send
        + Sync
        + 'static
{
}

impl<M: Held> MemoryRepository<M> {
    /// Runs `work` over the rows and the index of the row whose key is `id`, deleted softly or not.
    fn by_id<Id, R>(
        &self,
        id: &Id,
        work: impl FnOnce(&mut Vec<Row<M>>, Option<usize>) -> R,
    ) -> DatabaseResult<R>
    where
        Id: for<'q> Encode<'q, Postgres> + Type<Postgres>,
    {
        let id = cell_of(id)?;
        let key = column::<M>(M::PRIMARY_KEY)?;
        Ok(self.database.with_rows(|rows: &mut Vec<Row<M>>| {
            let at = rows
                .iter()
                .position(|row| row.cells.get(key).is_some_and(|cell| same(cell, &id)));
            work(rows, at)
        }))
    }
}

impl<M: Held> Repository<M, SqlDb> for MemoryRepository<M>
where
    M::Id: Send + Sync,
{
    type Id = M::Id;

    async fn insert(&self, model: &M) -> DatabaseResult<M> {
        let row = Row::of(model)?;
        let stored = row.model.clone();
        let key = key::<M>()?;
        let unique = unique_indexes::<M>()?;
        self.database.with_rows(|rows: &mut Vec<Row<M>>| {
            if rows
                .iter()
                .any(|held| same_key(&key, &held.cells, &row.cells))
            {
                return Err(DatabaseError::unique_violation(format!(
                    "{}_pkey",
                    M::TABLE
                )));
            }
            refuse_clash::<M>(&unique, &row.cells, others(rows, None))?;
            rows.push(row);
            Ok(stored)
        })
    }

    async fn update(&self, model: &M) -> DatabaseResult<M> {
        let row = Row::of(model)?;
        let stored = row.model.clone();
        let key = key::<M>()?;
        let unique = unique_indexes::<M>()?;
        self.database.with_rows(|rows: &mut Vec<Row<M>>| {
            let at = rows
                .iter()
                .position(|held| same_key(&key, &held.cells, &row.cells))
                .ok_or_else(|| DatabaseError::not_found(M::TYPE_NAME, "<key>"))?;
            replace(rows, at, row, &unique)?;
            Ok(stored)
        })
    }

    async fn upsert(&self, model: &M) -> DatabaseResult<M> {
        let row = Row::of(model)?;
        let stored = row.model.clone();
        let key = key::<M>()?;
        let unique = unique_indexes::<M>()?;
        self.database.with_rows(|rows: &mut Vec<Row<M>>| {
            match rows
                .iter()
                .position(|held| same_key(&key, &held.cells, &row.cells))
            {
                Some(at) => replace(rows, at, row, &unique)?,
                None => {
                    refuse_clash::<M>(&unique, &row.cells, others(rows, None))?;
                    rows.push(row);
                }
            }
            Ok(stored)
        })
    }

    fn query(&self) -> QueryBuilder<M, SqlDb> {
        QueryBuilder::new()
    }
}

impl<M: Held> KeyedRepository<M, SqlDb> for MemoryRepository<M>
where
    M::Id: Send + Sync + for<'q> Encode<'q, Postgres> + Type<Postgres>,
{
    async fn find(&self, id: &M::Id) -> DatabaseResult<Option<M>> {
        self.by_id(id, |rows, at| {
            at.and_then(|at| rows.get(at))
                .filter(|row| !row.deleted())
                .map(|row| row.model.clone())
        })
    }

    async fn find_required(&self, id: &M::Id) -> DatabaseResult<M> {
        self.find(id)
            .await?
            .ok_or_else(|| DatabaseError::not_found(M::TYPE_NAME, "<id>"))
    }

    async fn exists(&self, id: &M::Id) -> DatabaseResult<bool> {
        Ok(self.find(id).await?.is_some())
    }

    async fn delete(&self, id: &M::Id) -> DatabaseResult<bool> {
        self.by_id(id, |rows, at| at.map(|at| rows.remove(at)).is_some())
    }
}

impl<M: Held> RepositorySoftDelete<M, SqlDb> for MemoryRepository<M>
where
    M::Id: Send + Sync + for<'q> Encode<'q, Postgres> + Type<Postgres>,
{
    async fn soft_delete(&self, id: &M::Id) -> DatabaseResult<bool> {
        refuse_unless_soft::<M>()?;
        self.by_id(id, |rows, at| {
            let held = at.and_then(|at| rows.get_mut(at));
            match held.filter(|row| !row.deleted()) {
                Some(row) => {
                    row.delete_softly(Some(Utc::now()));
                    true
                }
                None => false,
            }
        })
    }

    async fn restore(&self, id: &M::Id) -> DatabaseResult<bool> {
        refuse_unless_soft::<M>()?;
        let unique = unique_indexes::<M>()?;
        self.by_id(id, |rows, at| {
            let Some((at, held)) = at.and_then(|at| Some((at, rows.get(at)?))) else {
                return Ok(false);
            };
            let mut restored = held.clone();
            restored.delete_softly(None);
            refuse_clash::<M>(&unique, &restored.cells, others(rows, Some(at)))?;
            if let Some(row) = rows.get_mut(at) {
                *row = restored;
            }
            Ok(true)
        })?
    }

    async fn find_with_deleted(&self, id: &M::Id) -> DatabaseResult<Option<M>> {
        self.by_id(id, |rows, at| {
            at.and_then(|at| rows.get(at)).map(|row| row.model.clone())
        })
    }

    async fn purge(&self, id: &M::Id) -> DatabaseResult<bool> {
        self.delete(id).await
    }
}

fn others<M>(
    rows: &[Row<M>],
    except: Option<usize>,
) -> impl Iterator<Item = &[FilterValue]> + Clone {
    rows.iter()
        .enumerate()
        .filter(move |(at, _)| Some(*at) != except)
        .map(|(_, row)| row.cells.as_slice())
}

/// Puts `row` at `at`, still deleted as it was, unless it clashes on a `unique` index.
fn replace<M: Held>(
    rows: &mut [Row<M>],
    at: usize,
    row: Row<M>,
    unique: &[crate::unique::Unique],
) -> DatabaseResult<()> {
    let Some(held) = rows.get(at) else {
        return Ok(());
    };
    let row = held.replaced(row);
    refuse_clash::<M>(unique, &row.cells, others(rows, Some(at)))?;
    if let Some(held) = rows.get_mut(at) {
        *held = row;
    }
    Ok(())
}

fn refuse_unless_soft<M: Held>() -> DatabaseResult<()> {
    if M::SOFT_DELETE {
        Ok(())
    } else {
        Err(DatabaseError::query(format!(
            "`{}` is not declared `#[model(soft_delete)]`",
            M::TYPE_NAME
        )))
    }
}

impl<M: Held> QueryExecutor<M, SqlDb> for MemoryRepository<M> {
    async fn execute<State>(&self, query: &QueryBuilder<M, SqlDb, State>) -> DatabaseResult<Vec<M>>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.database.with_rows(|rows: &mut Vec<Row<M>>| {
            let mut found = selected(rows, query.get_filters())?;
            ordered::<M>(&mut found, query.get_order_by())?;
            let page = paged(found, query.get_pagination());
            Ok(page.into_iter().map(|row| row.model).collect())
        })
    }

    async fn execute_one<State>(
        &self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<Option<M>>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        Ok(self.execute(query).await?.into_iter().next())
    }

    async fn count<State>(&self, query: &QueryBuilder<M, SqlDb, State>) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.database.with_rows(|rows: &mut Vec<Row<M>>| {
            let found = selected(rows, query.get_filters())?;
            Ok(u64::try_from(found.len()).unwrap_or(u64::MAX))
        })
    }

    async fn delete_where<State>(
        &self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let key = key::<M>()?;
        self.database.with_rows(|rows: &mut Vec<Row<M>>| {
            let mut found = chosen(rows, query.get_filters(), Deleted::Read)?;
            ordered::<M>(&mut found, query.get_order_by())?;
            let doomed = paged(found, query.get_pagination());
            let before = rows.len();
            rows.retain(|row| {
                !doomed
                    .iter()
                    .any(|gone| same_key(&key, &gone.cells, &row.cells))
            });
            Ok(u64::try_from(before - rows.len()).unwrap_or(u64::MAX))
        })
    }

    /// Refused: a row is held as its model; read the rows, change each and update it.
    async fn update_where<State>(
        &self,
        _query: &QueryBuilder<M, SqlDb, State>,
        _updates: &[(String, FilterValue)],
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        Err(DatabaseError::query(format!(
            "the memory database holds `{}` rows whole and updates no columns by filter: read the \
             rows, change each and update it",
            M::TYPE_NAME
        )))
    }
}
