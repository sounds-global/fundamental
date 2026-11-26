/// Whether an idempotent get-or-create write inserted a new row or found an existing one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    /// This call inserted the row (and consumed a sequence value).
    Inserted,
    /// A row already existed on the unique column; it was returned unchanged and consumed no sequence value.
    Existing,
}

impl WriteOutcome {
    /// Whether this call performed the insert.
    #[must_use]
    pub fn is_inserted(self) -> bool {
        matches!(self, Self::Inserted)
    }
}
