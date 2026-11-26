use chrono::{DateTime, Utc};
use uuid::Uuid;

/// What a lease row says: nobody holds it, or one holder does until an instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseState {
    /// Released, with the epoch its last holder held — the next holder's is higher.
    Vacant {
        /// The last holder's epoch.
        epoch: i64,
        /// When it was released.
        changed_at: DateTime<Utc>,
    },
    /// Held until `expires_at` unless renewed.
    Held {
        /// The fencing epoch of this tenure.
        epoch: i64,
        /// The holding process.
        holder: Uuid,
        /// When this tenure began.
        acquired_at: DateTime<Utc>,
        /// When another instance may take it.
        expires_at: DateTime<Utc>,
        /// When it was last written.
        changed_at: DateTime<Utc>,
    },
}

impl LeaseState {
    /// The epoch the row carries.
    #[must_use]
    pub fn epoch(&self) -> i64 {
        match *self {
            Self::Vacant { epoch, .. } | Self::Held { epoch, .. } => epoch,
        }
    }
}
