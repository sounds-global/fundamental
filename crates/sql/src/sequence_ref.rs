use crate::SqlResult;
use crate::ident::validate_identifier;

/// The single counter row that hands out a gap-free sequence for
/// [`SqlRepository::insert_or_get_sequenced`](crate::SqlRepository::insert_or_get_sequenced),
/// consumed only when the insert happens.
#[derive(Clone, Copy, Debug)]
pub struct SequenceRef<'a> {
    /// The counter table holding the single sequence row.
    pub table: &'a str,
    /// The column identifying the counter row (its primary key column).
    pub key_column: &'a str,
    /// The value of `key_column` selecting the one counter row.
    pub key: i64,
    /// The column holding the next index to assign.
    pub next_column: &'a str,
}

impl SequenceRef<'_> {
    /// Rejects any identifier that is not a plain SQL identifier.
    ///
    /// # Errors
    /// [`SqlError::Query`](crate::SqlError) if `table`, `key_column`, or
    /// `next_column` is not a valid identifier.
    pub(crate) fn validate(&self) -> SqlResult<()> {
        validate_identifier(self.table, false)?;
        validate_identifier(self.key_column, false)?;
        validate_identifier(self.next_column, false)?;
        Ok(())
    }
}
