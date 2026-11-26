//! The [`IndexKey`] handle — a secondary index a model declares, typed by that model.

use core::fmt;
use core::marker::PhantomData;

/// A secondary index declared on model `M` with `#[model(index(...))]`, emitted as a constant on the model.
///
/// The type parameter keeps an index of one model from addressing a query over another. It does not create the
/// index; the migration does, under the same name.
pub struct IndexKey<M> {
    name: &'static str,
    columns: &'static [&'static str],
    _model: PhantomData<fn() -> M>,
}

impl<M> IndexKey<M> {
    /// The handle for an index the derive has checked against `M`'s own columns.
    #[doc(hidden)]
    #[must_use]
    pub const fn declared(name: &'static str, columns: &'static [&'static str]) -> Self {
        Self {
            name,
            columns,
            _model: PhantomData,
        }
    }

    /// The index's name, as the migration creates it.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The index's columns, in index order.
    #[must_use]
    pub const fn columns(&self) -> &'static [&'static str] {
        self.columns
    }

    /// The column the index is ordered by first, the only one a query can seek on alone.
    ///
    /// # Panics
    ///
    /// Never for a handle the derive emitted: it refuses an index with no columns at expansion.
    #[must_use]
    pub const fn leading_column(&self) -> &'static str {
        self.columns[0]
    }
}

// Hand-written: a derive would bound the impl on `M`.
impl<M> Clone for IndexKey<M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M> Copy for IndexKey<M> {}

impl<M> fmt::Debug for IndexKey<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IndexKey")
            .field("name", &self.name)
            .field("columns", &self.columns)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::IndexKey;

    struct Membership;

    const BY_ACCOUNT: IndexKey<Membership> = IndexKey::declared(
        "memberships_by_account",
        &["staff_account_id", "organisation_id"],
    );

    #[test]
    fn the_leading_column_is_the_first_declared() {
        assert_eq!(BY_ACCOUNT.name(), "memberships_by_account");
        assert_eq!(BY_ACCOUNT.leading_column(), "staff_account_id");
        assert_eq!(
            BY_ACCOUNT.columns(),
            ["staff_account_id", "organisation_id"]
        );
    }
}
