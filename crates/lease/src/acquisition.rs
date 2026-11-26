use chrono::{DateTime, Utc};

use crate::HeldLease;

/// What an attempt to take or renew a lease found.
#[derive(Debug)]
pub enum Acquisition {
    /// This process holds the lease — a tenure begun or renewed.
    Held(HeldLease),
    /// Another holder's tenure is live: the normal outcome for every instance but one.
    HeldElsewhere {
        /// Its epoch.
        epoch: i64,
        /// When, by the database's clock, it may be taken.
        expires_at: DateTime<Utc>,
    },
}
