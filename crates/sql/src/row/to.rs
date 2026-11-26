//! Mapping model types onto SQL columns and binding their values to queries.

use fundamental_encryption::KeyManager;

use super::BindValues;
use crate::SqlResult;

/// Maps a model type onto its SQL columns and binds its field values in column order; the first column is the primary key.
pub trait ToRow {
    /// Returns the column names for this type.
    fn columns() -> &'static [&'static str];

    /// Returns the column names excluding the primary key.
    fn columns_without_pk() -> &'static [&'static str] {
        let cols = Self::columns();
        if cols.is_empty() {
            cols
        } else {
            &cols[1..] // Assume first column is PK
        }
    }

    /// Returns the number of columns.
    fn column_count() -> usize {
        Self::columns().len()
    }

    /// Binds this instance's values to a query.
    fn bind_to<'q, Q>(&'q self, query: Q) -> Q
    where
        Q: BindValues<'q>;

    /// Binds this instance's values excluding the primary key.
    fn bind_without_pk<'q, Q>(&'q self, query: Q) -> Q
    where
        Q: BindValues<'q>;

    /// Binds exactly the primary-key columns, in key order, so a composite key is matched in full.
    ///
    /// No default: one that bound nothing would leave a hand-written impl a placeholder short.
    fn bind_key<'q, Q>(&'q self, query: Q) -> Q
    where
        Q: BindValues<'q>;

    /// Binds the column at `column_index` in `columns_without_pk()`, for batch UPDATE statements.
    fn bind_column_for_batch<'q, Q>(&'q self, query: Q, _column_index: usize) -> Q
    where
        Q: BindValues<'q>,
    {
        query
    }

    /// Like [`bind_to`](ToRow::bind_to), but encrypts any `Secret` columns using `enc`.
    ///
    /// # Errors
    ///
    /// Returns an error if a `Secret` column needs encryption but `enc` is `None`,
    /// or if encryption fails.
    fn bind_to_encrypted<'q, Q>(&'q self, query: Q, enc: Option<&KeyManager>) -> SqlResult<Q>
    where
        Q: BindValues<'q>,
    {
        let _ = enc;
        Ok(self.bind_to(query))
    }

    /// Like [`bind_without_pk`](ToRow::bind_without_pk), but encrypts any `Secret` columns using `enc`.
    ///
    /// # Errors
    ///
    /// Returns an error if a `Secret` column needs encryption but `enc` is `None`,
    /// or if encryption fails.
    fn bind_without_pk_encrypted<'q, Q>(
        &'q self,
        query: Q,
        enc: Option<&KeyManager>,
    ) -> SqlResult<Q>
    where
        Q: BindValues<'q>,
    {
        let _ = enc;
        Ok(self.bind_without_pk(query))
    }

    /// Like [`bind_key`](ToRow::bind_key), but encrypts any `Secret` key column using `enc`.
    ///
    /// # Errors
    ///
    /// Returns an error if a key column needs encryption but `enc` is `None`, or if
    /// encryption fails.
    fn bind_key_encrypted<'q, Q>(&'q self, query: Q, enc: Option<&KeyManager>) -> SqlResult<Q>
    where
        Q: BindValues<'q>,
    {
        let _ = enc;
        Ok(self.bind_key(query))
    }

    /// Like [`bind_column_for_batch`](ToRow::bind_column_for_batch), but encrypts a `Secret` column with `enc`.
    ///
    /// # Errors
    ///
    /// Returns an error if the column needs encryption but `enc` is `None`, or if
    /// encryption fails.
    fn bind_column_for_batch_encrypted<'q, Q>(
        &'q self,
        query: Q,
        column_index: usize,
        enc: Option<&KeyManager>,
    ) -> SqlResult<Q>
    where
        Q: BindValues<'q>,
    {
        let _ = enc;
        Ok(self.bind_column_for_batch(query, column_index))
    }
}
