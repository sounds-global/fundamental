/// What releasing a lease found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Released {
    /// This process held it; it is now vacant, keeping `epoch` so the next holder's is higher.
    Vacated {
        /// The tenure that ended.
        epoch: i64,
    },
    /// This process did not hold it, so nothing changed.
    NotHeld,
}
