//! The [`Offline`] database marker.

use super::Database;
use crate::Sealed;

/// Marker type for the offline backend: the local SQLite and IndexedDB store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Offline;

impl Sealed for Offline {}

impl Database for Offline {
    const NAME: &'static str = "Offline";
    const SUPPORTS_TRANSACTIONS: bool = false;
    const SUPPORTS_JOINS: bool = false;
    const SUPPORTS_SECONDARY_INDEXES: bool = false;
}
