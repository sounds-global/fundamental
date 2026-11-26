//! [`OutboxClaim`]: the rows one claim took, and what it did with those it would not take.

use fundamental_core::outbox::SetAsideTally;

/// What one claim did with the due rows it read.
///
/// A row that refused to decode is never in `rows`: it is counted in `set_aside` or `skipped`.
#[derive(Debug)]
pub struct OutboxClaim<R> {
    /// The decoded rows, each leased to this worker.
    pub rows: Vec<R>,
    /// Rows set aside in the claiming transaction, by why.
    pub set_aside: SetAsideTally,
    /// Rows this process could not decode with the keys it holds, left for another process.
    pub skipped: u64,
}

impl<R> OutboxClaim<R> {
    /// A claim that took nothing and set nothing aside.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            rows: Vec::new(),
            set_aside: SetAsideTally::default(),
            skipped: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OutboxClaim;

    #[test]
    fn an_empty_claim_holds_nothing() {
        let claim = OutboxClaim::<u8>::empty();
        assert!(claim.rows.is_empty());
        assert_eq!(claim.set_aside.total(), 0);
        assert_eq!(claim.skipped, 0);
    }
}
