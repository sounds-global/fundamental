/// How a `SELECT` locks the rows it returns.
///
/// Crate-private: YSQL ignores `SKIP LOCKED` under `SERIALIZABLE` without failing, so each
/// caller here pairs the clause with the isolation it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum RowLock {
    /// `FOR UPDATE SKIP LOCKED`: lock the returned rows, passing over any another transaction holds.
    ///
    /// A row committed moments ago may still carry a write intent and be skipped; poll again
    /// rather than concluding it is absent.
    ForUpdateSkipLocked,

    /// `FOR UPDATE`: lock the returned rows, waiting for any another transaction holds.
    ///
    /// For locking one row chosen by a predicate as a transaction's first statement; a separate
    /// unlocked read first makes contention surface as a deadlock.
    ForUpdate,
}

impl RowLock {
    /// The SQL clause, appended after `LIMIT`/`OFFSET`.
    #[must_use]
    pub(crate) const fn to_sql(self) -> &'static str {
        match self {
            Self::ForUpdateSkipLocked => "FOR UPDATE SKIP LOCKED",
            Self::ForUpdate => "FOR UPDATE",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_waiting_clause() {
        assert_eq!(RowLock::ForUpdate.to_sql(), "FOR UPDATE");
    }

    #[test]
    fn renders_the_skip_locked_clause() {
        assert_eq!(
            RowLock::ForUpdateSkipLocked.to_sql(),
            "FOR UPDATE SKIP LOCKED"
        );
    }
}
