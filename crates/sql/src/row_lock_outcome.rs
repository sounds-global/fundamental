/// What [`SqlTransaction::find_for_update_unless_held`](crate::SqlTransaction::find_for_update_unless_held)
/// found: the row locked for this transaction, the row held by another, or no row.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum RowLockOutcome<M> {
    /// The row, locked for the rest of this transaction.
    Locked(M),
    /// Another transaction holds the row's lock; the row is as this transaction reads it, unlocked.
    HeldElsewhere(M),
    /// No row has the key.
    Absent,
}

impl<M> RowLockOutcome<M> {
    /// The row, locked or not, or `None` when there is none.
    pub fn row(&self) -> Option<&M> {
        match self {
            Self::Locked(row) | Self::HeldElsewhere(row) => Some(row),
            Self::Absent => None,
        }
    }

    /// Whether this transaction holds the row's lock.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        matches!(self, Self::Locked(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_locked_row_is_held_by_this_transaction() {
        assert!(RowLockOutcome::Locked(1).is_locked());
        assert!(!RowLockOutcome::HeldElsewhere(1).is_locked());
        assert!(!RowLockOutcome::<u8>::Absent.is_locked());
    }

    #[test]
    fn a_held_row_is_still_read() {
        assert_eq!(RowLockOutcome::Locked(7).row(), Some(&7));
        assert_eq!(RowLockOutcome::HeldElsewhere(8).row(), Some(&8));
        assert_eq!(RowLockOutcome::<u8>::Absent.row(), None);
    }
}
