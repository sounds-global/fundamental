use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use fundamental_db::{DatabaseResult, FilterValue};
use parking_lot::Mutex;

use chrono::{DateTime, Utc};

use crate::cells::cells_of;
use crate::{Held, MemoryRepository};

/// Every model's rows, held in memory and shared by each clone.
#[derive(Clone, Default)]
pub struct MemoryDatabase {
    tables: Arc<Mutex<HashMap<TypeId, Box<dyn Any + Send>>>>,
}

/// A row as held: the model, its cells in column order, then when it was deleted for a soft-deleted model.
#[derive(Clone)]
pub(crate) struct Row<M> {
    pub(crate) model: M,
    pub(crate) cells: Vec<FilterValue>,
}

impl<M: Held> Row<M> {
    /// The row `model` is written as, not deleted.
    ///
    /// # Errors
    ///
    /// [`DatabaseError::Configuration`](fundamental_db::DatabaseError) when a value cannot be bound.
    pub(crate) fn of(model: &M) -> DatabaseResult<Self> {
        let model = model.clone().into_stored();
        let mut cells = cells_of(&model)?;
        if M::SOFT_DELETE {
            cells.push(FilterValue::Null);
        }
        Ok(Self { model, cells })
    }

    pub(crate) fn deleted(&self) -> bool {
        M::SOFT_DELETE
            && self
                .cells
                .get(M::columns().len())
                .is_some_and(|cell| !matches!(cell, FilterValue::Null))
    }

    /// Marks the row deleted softly at `at`, or not deleted for `None`.
    pub(crate) fn delete_softly(&mut self, at: Option<DateTime<Utc>>) {
        if let Some(cell) = self.cells.get_mut(M::columns().len()) {
            *cell = at.map_or(FilterValue::Null, FilterValue::TimestampTz);
        }
    }

    /// The row holding `replacement`'s model, still deleted as this one was.
    pub(crate) fn replaced(&self, replacement: Self) -> Self {
        let mut row = replacement;
        let at = M::columns().len();
        if let (Some(kept), Some(cell)) = (self.cells.get(at), row.cells.get_mut(at)) {
            cell.clone_from(kept);
        }
        row
    }
}

impl MemoryDatabase {
    /// A database that holds no rows.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The repository of `M`'s rows.
    #[must_use]
    pub fn repository<M>(&self) -> MemoryRepository<M> {
        MemoryRepository::new(self.clone())
    }

    /// Runs `work` over `M`'s rows, one writer or reader at a time.
    pub(crate) fn with_rows<M: Send + 'static, R>(
        &self,
        work: impl FnOnce(&mut Vec<Row<M>>) -> R,
    ) -> R {
        let mut tables = self.tables.lock();
        let table = tables
            .entry(TypeId::of::<M>())
            .or_insert_with(|| Box::new(Vec::<Row<M>>::new()));
        match table.downcast_mut::<Vec<Row<M>>>() {
            Some(rows) => work(rows),
            // Each table is keyed by its own model's type, so the downcast always holds.
            None => work(&mut Vec::new()),
        }
    }
}

impl std::fmt::Debug for MemoryDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryDatabase")
            .field("tables", &self.tables.lock().len())
            .finish()
    }
}

impl fundamental_sql::Repositories for MemoryDatabase {
    type Repository<M>
        = MemoryRepository<M>
    where
        M: fundamental_sql::Stored;

    fn repository<M: fundamental_sql::Stored>(&self) -> MemoryRepository<M> {
        Self::repository(self)
    }
}
