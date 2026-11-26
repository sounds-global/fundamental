//! The [`OrderBy`] ordering clause.

use super::{OrderDirection, OrderNulls};

/// An ordering clause.
#[derive(Debug, Clone)]
pub struct OrderBy {
    /// The column to order by.
    pub column: String,
    /// The direction.
    pub direction: OrderDirection,
    /// Nulls handling.
    pub nulls: Option<OrderNulls>,
}

impl OrderBy {
    /// Creates an ascending order.
    pub fn asc(column: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            direction: OrderDirection::Asc,
            nulls: None,
        }
    }

    /// Creates a descending order.
    pub fn desc(column: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            direction: OrderDirection::Desc,
            nulls: None,
        }
    }

    /// Sets nulls to appear first.
    pub fn nulls_first(mut self) -> Self {
        self.nulls = Some(OrderNulls::First);
        self
    }

    /// Sets nulls to appear last.
    pub fn nulls_last(mut self) -> Self {
        self.nulls = Some(OrderNulls::Last);
        self
    }
}
