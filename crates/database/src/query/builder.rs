//! The type-safe [`QueryBuilder`]; building performs no I/O.

use super::{Filter, FilterValue, OrderBy, Pagination};
use core::marker::PhantomData;
use fundamental_core::model::{
    BoundTo, Database, HasLeadingKey, IndexKey, NoLeadingKey, SingleKeyModel, SqlModel,
};

/// A type-safe query builder.
///
/// `State` is [`NoLeadingKey`] (the default) or [`HasLeadingKey`], reached through
/// [`pin_key`](Self::pin_key). Execution requires the
/// [`LeadingKeyAddressed`](fundamental_core::model::LeadingKeyAddressed) witness, so a query
/// over a composite-key model cannot run unpinned and scan every value of the leading column.
#[derive(Debug)]
pub struct QueryBuilder<M, D, State = NoLeadingKey>
where
    M: BoundTo<D>,
    D: Database,
{
    filters: Vec<Filter>,
    order_by: Vec<OrderBy>,
    pagination: Option<Pagination>,
    select: Option<Vec<String>>,
    _model: PhantomData<M>,
    _database: PhantomData<D>,
    /// `fn() -> State`, so the tag cannot change the builder's auto traits.
    _state: PhantomData<fn() -> State>,
}

impl<M, D> QueryBuilder<M, D, NoLeadingKey>
where
    M: BoundTo<D> + SingleKeyModel,
    D: Database,
{
    /// States that this query already addresses rows: the model's whole key is one column.
    ///
    /// For a surface that must accept both shapes in one return type. The bound is
    /// [`SingleKeyModel`], the same condition
    /// [`LeadingKeyAddressed`](fundamental_core::model::LeadingKeyAddressed) already accepts.
    #[must_use]
    pub fn already_addressed(self) -> QueryBuilder<M, D, HasLeadingKey> {
        QueryBuilder {
            filters: self.filters,
            order_by: self.order_by,
            pagination: self.pagination,
            select: self.select,
            _model: PhantomData,
            _database: PhantomData,
            _state: PhantomData,
        }
    }
}

impl<M, D> QueryBuilder<M, D, NoLeadingKey>
where
    M: BoundTo<D> + SqlModel,
    D: Database,
{
    /// Pins the query's **leading key column** to one value, so it can be executed.
    ///
    /// The column is the model's own leading [`KEY_COLUMNS`](SqlModel::KEY_COLUMNS) entry, so the
    /// caller cannot pin the wrong one.
    #[must_use]
    pub fn pin_key(mut self, value: impl Into<FilterValue>) -> QueryBuilder<M, D, HasLeadingKey> {
        self.filters
            .push(Filter::eq(M::KEY_COLUMNS[0].to_string(), value));
        QueryBuilder {
            filters: self.filters,
            order_by: self.order_by,
            pagination: self.pagination,
            select: self.select,
            _model: PhantomData,
            _database: PhantomData,
            _state: PhantomData,
        }
    }

    /// Pins the leading column of a secondary index the model declares, so the query can be
    /// executed through that index.
    ///
    /// Reaches the same [`HasLeadingKey`] state as [`pin_key`](Self::pin_key). [`IndexKey<M>`] is
    /// emitted only by the model's own derive, so another table's index does not type-check.
    #[must_use]
    pub fn pin_index(
        mut self,
        index: IndexKey<M>,
        value: impl Into<FilterValue>,
    ) -> QueryBuilder<M, D, HasLeadingKey> {
        self.filters
            .push(Filter::eq(index.leading_column().to_string(), value));
        QueryBuilder {
            filters: self.filters,
            order_by: self.order_by,
            pagination: self.pagination,
            select: self.select,
            _model: PhantomData,
            _database: PhantomData,
            _state: PhantomData,
        }
    }
}

impl<M, D> QueryBuilder<M, D, NoLeadingKey>
where
    M: BoundTo<D>,
    D: Database,
{
    /// Creates a new query builder.
    ///
    /// Not generic over the state, so the pinned state cannot be minted through a turbofish.
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
            order_by: Vec::new(),
            pagination: None,
            select: None,
            _model: PhantomData,
            _database: PhantomData,
            _state: PhantomData,
        }
    }
}

impl<M, D, State> QueryBuilder<M, D, State>
where
    M: BoundTo<D>,
    D: Database,
{
    /// Adds a filter condition.
    pub fn filter(mut self, filter: Filter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Adds multiple filter conditions (AND).
    pub fn filters(mut self, filters: impl IntoIterator<Item = Filter>) -> Self {
        self.filters.extend(filters);
        self
    }

    /// Adds an ordering clause.
    pub fn order_by(mut self, order: OrderBy) -> Self {
        self.order_by.push(order);
        self
    }

    /// Sets the maximum number of results.
    pub fn limit(mut self, limit: u64) -> Self {
        self.pagination = Some(match self.pagination {
            Some(p) => Pagination { limit, ..p },
            None => Pagination::new(limit, 0),
        });
        self
    }

    /// Sets the offset for pagination.
    pub fn offset(mut self, offset: u64) -> Self {
        self.pagination = Some(match self.pagination {
            Some(p) => Pagination { offset, ..p },
            None => Pagination::new(u64::MAX, offset),
        });
        self
    }

    /// Sets pagination (limit and offset).
    pub fn paginate(mut self, pagination: Pagination) -> Self {
        self.pagination = Some(pagination);
        self
    }

    /// Selects specific columns.
    pub fn select(mut self, columns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.select = Some(columns.into_iter().map(Into::into).collect());
        self
    }

    /// Returns the filters.
    pub fn get_filters(&self) -> &[Filter] {
        &self.filters
    }

    /// Returns the ordering clauses.
    pub fn get_order_by(&self) -> &[OrderBy] {
        &self.order_by
    }

    /// Returns the pagination settings.
    pub fn get_pagination(&self) -> Option<&Pagination> {
        self.pagination.as_ref()
    }

    /// Returns the selected columns.
    pub fn get_select(&self) -> Option<&[String]> {
        self.select.as_deref()
    }

    /// Clears all filters.
    pub fn clear_filters(mut self) -> Self {
        self.filters.clear();
        self
    }

    /// Clears all ordering.
    pub fn clear_order_by(mut self) -> Self {
        self.order_by.clear();
        self
    }
}

impl<M, D> Default for QueryBuilder<M, D, NoLeadingKey>
where
    M: BoundTo<D>,
    D: Database,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<M, D, State> Clone for QueryBuilder<M, D, State>
where
    M: BoundTo<D>,
    D: Database,
{
    /// Cloning preserves the pinned state.
    fn clone(&self) -> Self {
        Self {
            filters: self.filters.clone(),
            order_by: self.order_by.clone(),
            pagination: self.pagination,
            select: self.select.clone(),
            _model: PhantomData,
            _database: PhantomData,
            _state: PhantomData,
        }
    }
}
