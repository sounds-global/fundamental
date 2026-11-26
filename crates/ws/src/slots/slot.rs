//! [`Slot`]: the one permit a connection holds, and the state it holds it in.

use tokio::sync::OwnedSemaphorePermit;

use crate::session::CloseReason;
use crate::slots::Pool;

/// A connection's slot; every live state holds its permit, released only by moving to [`Closed`](Self::Closed).
#[derive(Debug)]
pub(crate) enum Slot {
    /// Accepted and no principal bound, holding an anonymous permit.
    Anonymous(OwnedSemaphorePermit),
    /// Admitted to `pool`, counted against `holder` once a principal is bound.
    Admitted {
        pool: Pool,
        _permit: OwnedSemaphorePermit,
        holder: Option<String>,
    },
    /// Told to end for `reason`, still holding its permit until its socket is gone.
    Ending {
        reason: CloseReason,
        pool: Pool,
        _permit: OwnedSemaphorePermit,
    },
    /// Ended, with its permit given back.
    Closed,
}

impl Slot {
    /// The pool whose permit this slot holds, if it still holds one.
    pub(crate) fn pool(&self) -> Option<Pool> {
        match self {
            Self::Anonymous(_) => Some(Pool::Anonymous),
            Self::Admitted { pool, .. } | Self::Ending { pool, .. } => Some(*pool),
            Self::Closed => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    #[test]
    fn each_live_state_names_its_pool() {
        let semaphore = Arc::new(Semaphore::new(3));
        let permit = || semaphore.clone().try_acquire_owned().expect("a permit");
        assert_eq!(Slot::Anonymous(permit()).pool(), Some(Pool::Anonymous));
        assert_eq!(
            Slot::Admitted {
                pool: Pool::Authenticated,
                _permit: permit(),
                holder: None,
            }
            .pool(),
            Some(Pool::Authenticated)
        );
        assert_eq!(
            Slot::Ending {
                reason: CloseReason::Displaced,
                pool: Pool::Anonymous,
                _permit: permit()
            }
            .pool(),
            Some(Pool::Anonymous)
        );
        assert_eq!(Slot::Closed.pool(), None);
    }
}
