//! [`Held`], the outcome of holding findings against a ledger.

use super::Unheld;

/// What holding a check's findings against its ledger came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held<F> {
    /// How many findings an exact entry holds.
    pub frozen: usize,
    /// Everything the ledger did not hold; the check is green only when this is empty.
    pub unheld: Vec<Unheld<F>>,
}

impl<F> Default for Held<F> {
    fn default() -> Self {
        Self {
            frozen: 0,
            unheld: Vec::new(),
        }
    }
}

impl<F> Held<F> {
    /// Whether the ledger held every finding and every entry matched.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.unheld.is_empty()
    }
}
