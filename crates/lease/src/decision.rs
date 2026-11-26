use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{LeaseError, LeaseState};

/// What an acquirer does, given the row it locked and the database's time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Begin a new tenure at `epoch`.
    Begin {
        /// The new fencing epoch.
        epoch: i64,
    },
    /// This holder's live tenure: extend it at the same epoch.
    Renew {
        /// The tenure's epoch.
        epoch: i64,
    },
    /// Another holder's live tenure: leave it.
    Refuse {
        /// Its epoch.
        epoch: i64,
        /// When it may be taken.
        expires_at: DateTime<Utc>,
    },
}

impl Decision {
    /// Decides for holder `me` at `now` over `current`, the row as locked (or `None`).
    ///
    /// A new epoch is `max(previous + 1, now in microseconds)`, so it rises even after a restore.
    pub(crate) fn decide(
        lease: &str,
        current: Option<&LeaseState>,
        me: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Self, LeaseError> {
        match current {
            Some(&LeaseState::Held {
                epoch,
                holder,
                expires_at,
                ..
            }) if expires_at > now => {
                if holder == me {
                    Ok(Self::Renew { epoch })
                } else {
                    Ok(Self::Refuse { epoch, expires_at })
                }
            }
            _ => {
                let floor = current
                    .map(|state| state.epoch().checked_add(1))
                    .unwrap_or(Some(1))
                    .ok_or_else(|| LeaseError::EpochExhausted {
                        lease: lease.to_owned(),
                    })?;
                Ok(Self::Begin {
                    epoch: floor.max(now.timestamp_micros()).max(1),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use fundamental_core::Error as _;

    use super::*;

    fn at(micros: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_micros(micros).expect("an instant")
    }

    fn held(epoch: i64, holder: Uuid, expires: i64) -> LeaseState {
        LeaseState::Held {
            epoch,
            holder,
            acquired_at: at(0),
            expires_at: at(expires),
            changed_at: at(0),
        }
    }

    #[test]
    fn the_decision_table_is_total() {
        let (me, other) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let now = at(1_000);
        let decide =
            |state: Option<LeaseState>| Decision::decide("keeper", state.as_ref(), me, now);

        assert_eq!(decide(None).ok(), Some(Decision::Begin { epoch: 1_000 }));
        assert_eq!(
            decide(Some(held(7, me, 2_000))).ok(),
            Some(Decision::Renew { epoch: 7 })
        );
        assert_eq!(
            decide(Some(held(7, other, 2_000))).ok(),
            Some(Decision::Refuse {
                epoch: 7,
                expires_at: at(2_000)
            })
        );
        assert_eq!(
            decide(Some(held(5_000, me, 900))).ok(),
            Some(Decision::Begin { epoch: 5_001 }),
            "my own expired tenure is over; the next one is new"
        );
        assert_eq!(
            decide(Some(held(7, other, 1_000))).ok(),
            Some(Decision::Begin { epoch: 1_000 })
        );
        assert_eq!(
            decide(Some(LeaseState::Vacant {
                epoch: 9_000,
                changed_at: at(0)
            }))
            .ok(),
            Some(Decision::Begin { epoch: 9_001 })
        );
    }

    #[test]
    fn the_epoch_never_overflows_silently() {
        let error = Decision::decide(
            "keeper",
            Some(&LeaseState::Vacant {
                epoch: i64::MAX,
                changed_at: at(0),
            }),
            Uuid::nil(),
            at(0) + TimeDelta::seconds(1),
        )
        .expect_err("no epoch above i64::MAX");
        assert_eq!(error.error_code(), "SLEASE_007");
    }
}
