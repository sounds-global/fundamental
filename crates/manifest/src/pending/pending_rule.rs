//! [`PendingRule`], a rule whose findings a ledger may freeze.

use std::fmt::Display;

use fundamental_core::Variant;

/// A rule judged one file at a time, whose existing findings a [`PendingLedger`] may freeze.
///
/// [`PendingLedger`]: super::PendingLedger
pub trait PendingRule: Variant + Ord + Display {
    /// The key (`crate` or `item`) a frozen finding names beside its file, so debt toward one
    /// thing cannot be swapped for another's; `None` when the file alone keys it.
    fn name_key(self) -> Option<&'static str> {
        None
    }
}
