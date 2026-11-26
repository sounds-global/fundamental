//! [`Selection`]: what `fun test` runs when it is given no filter.

use std::collections::BTreeSet;

/// What `fun test` runs when it is given no filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Selection {
    /// Every test: a change reached a file no package's tests are known by.
    Everything,
    /// The tests of these workspace members, by name; none when nothing changed.
    Packages(BTreeSet<String>),
}
