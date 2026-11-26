use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_db::{
    DatabaseError, IsolationLevel, KeyedRepository as _, RepositoryTransaction as _,
};
use fundamental_sql::{ClusterTime, SqlPool, SqlTransaction};
use tokio::time::Instant;
use tracing::{debug, instrument, warn};

use crate::decision::Decision;
use crate::{
    Acquisition, HeldLease, LeaseConfig, LeaseError, LeaseHolder, LeaseName, LeaseRow, LeaseState,
    Released,
};

/// One process's handle on one singleton's lease, in one database.
#[derive(Clone)]
pub struct Lease(Arc<Parts>);

struct Parts {
    pool: SqlPool,
    name: LeaseName,
    holder: LeaseHolder,
    config: LeaseConfig,
}

impl Lease {
    /// A handle on the lease `name` in the database `pool` reaches.
    #[must_use]
    pub fn new(pool: SqlPool, name: LeaseName, config: LeaseConfig) -> Self {
        Self(Arc::new(Parts {
            pool,
            name,
            holder: LeaseHolder::generate(),
            config,
        }))
    }

    /// Which lease.
    #[must_use]
    pub fn name(&self) -> LeaseName {
        self.0.name
    }

    /// This handle's holder.
    #[must_use]
    pub fn holder(&self) -> LeaseHolder {
        self.0.holder
    }

    /// Its timings.
    #[must_use]
    pub fn config(&self) -> LeaseConfig {
        self.0.config
    }

    pub(crate) fn pool(&self) -> &SqlPool {
        &self.0.pool
    }

    /// Takes the lease if it is free or expired, renews it if this handle holds it, and
    /// reports who holds it otherwise.
    ///
    /// # Errors
    ///
    /// [`LeaseError::Unavailable`], [`LeaseError::ClockUnavailable`],
    /// [`LeaseError::TableMissing`], [`LeaseError::CommitOutcomeUnknown`],
    /// [`LeaseError::CorruptRow`] or [`LeaseError::EpochExhausted`].
    #[instrument(name = "lease.acquire", skip(self), fields(lease = %self.name(), holder = %self.holder()), err(Debug))]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    pub async fn try_acquire(&self) -> Result<Acquisition, LeaseError> {
        let started = Instant::now();
        let name = self.name().as_str();
        let me = self.holder().as_uuid();
        let ttl = TimeDelta::from_std(self.config().ttl()).map_err(|_| LeaseError::CorruptRow {
            lease: name.to_owned(),
            detail: "a TTL beyond a timestamp's range".to_owned(),
        })?;
        let step = self
            .pool()
            .transaction(IsolationLevel::Serializable, async |tx| {
                let row = tx.find_for_update::<LeaseRow>(&name.to_owned()).await?;
                let now = cluster_now(tx, name).await?;
                let state = row.as_ref().map(LeaseRow::state).transpose()?;
                match Decision::decide(name, state.as_ref(), me, now)? {
                    Decision::Refuse { epoch, expires_at } => {
                        Ok(Step::Elsewhere { epoch, expires_at })
                    }
                    Decision::Renew { epoch } => {
                        let acquired_at = match state {
                            Some(LeaseState::Held { acquired_at, .. }) => acquired_at,
                            _ => now,
                        };
                        let expires_at = expiry(name, now, ttl)?;
                        let renewed = LeaseState::Held {
                            epoch,
                            holder: me,
                            acquired_at,
                            expires_at,
                            changed_at: now,
                        };
                        tx.update(&LeaseRow::recording(name, &renewed)).await?;
                        Ok(Step::Held {
                            epoch,
                            expires_at,
                            began: None,
                        })
                    }
                    Decision::Begin { epoch } => {
                        let expires_at = expiry(name, now, ttl)?;
                        let begun = LeaseState::Held {
                            epoch,
                            holder: me,
                            acquired_at: now,
                            expires_at,
                            changed_at: now,
                        };
                        let written = LeaseRow::recording(name, &begun);
                        if row.is_some() {
                            tx.update(&written).await?;
                        } else if tx.insert_if_absent(&written).await?.is_none() {
                            // A first acquirer committed between our read and our insert.
                            return Ok(Step::Elsewhere {
                                epoch: 0,
                                expires_at: now,
                            });
                        }
                        let gap = state.map(|previous| match previous {
                            LeaseState::Vacant { changed_at, .. } => now - changed_at,
                            LeaseState::Held { expires_at, .. } => now - expires_at,
                        });
                        Ok(Step::Held {
                            epoch,
                            expires_at,
                            began: Some(gap),
                        })
                    }
                }
            })
            .await
            .map_err(|error: LeaseError| error.about(name))?;
        Ok(match step {
            Step::Elsewhere { epoch, expires_at } => {
                debug!(epoch, %expires_at, "lease held elsewhere");
                Acquisition::HeldElsewhere { epoch, expires_at }
            }
            Step::Held {
                epoch,
                expires_at,
                began,
            } => {
                let local = self
                    .config()
                    .ttl()
                    .saturating_sub(self.config().max_clock_skew());
                Acquisition::Held(HeldLease::new(
                    self.clone(),
                    epoch,
                    expires_at,
                    started + local,
                    began.map(|gap| gap.and_then(|gap| gap.to_std().ok())),
                ))
            }
        })
    }

    /// Ends this handle's tenure, if it holds one, leaving the row vacant at the same epoch.
    ///
    /// # Errors
    ///
    /// As [`try_acquire`](Self::try_acquire).
    #[instrument(name = "lease.release", skip(self), fields(lease = %self.name(), holder = %self.holder()), err(Debug))]
    pub async fn release(&self) -> Result<Released, LeaseError> {
        let name = self.name().as_str();
        let me = self.holder().as_uuid();
        self.pool()
            .transaction(IsolationLevel::Serializable, async |tx| {
                let Some(row) = tx.find_for_update::<LeaseRow>(&name.to_owned()).await? else {
                    return Ok(Released::NotHeld);
                };
                let now = cluster_now(tx, name).await?;
                match row.state()? {
                    LeaseState::Held { epoch, holder, .. } if holder == me => {
                        let vacant = LeaseState::Vacant {
                            epoch,
                            changed_at: now,
                        };
                        tx.update(&LeaseRow::recording(name, &vacant)).await?;
                        Ok(Released::Vacated { epoch })
                    }
                    _ => Ok(Released::NotHeld),
                }
            })
            .await
            .map_err(|error: LeaseError| error.about(name))
    }

    /// The lease's row as it stands, unlocked, and the database's time now.
    ///
    /// # Errors
    ///
    /// [`LeaseError::Unavailable`], [`LeaseError::TableMissing`] or
    /// [`LeaseError::CorruptRow`].
    #[instrument(name = "lease.current", skip(self), fields(lease = %self.name(), holder = %self.holder()), err(Debug))]
    pub async fn current(&self) -> Result<(Option<LeaseState>, DateTime<Utc>), LeaseError> {
        let name = self.name().as_str();
        let row = self
            .pool()
            .repository::<LeaseRow>()
            .find(&name.to_owned())
            .await
            .map_err(|error| LeaseError::Store(error).about(name))?;
        let now = ClusterTime::read(self.pool())
            .await
            .map_err(|error| LeaseError::ClockUnavailable {
                lease: name.to_owned(),
                cause: DatabaseError::from(error),
            })?
            .to_utc();
        Ok((row.as_ref().map(LeaseRow::state).transpose()?, now))
    }
}

impl fmt::Debug for Lease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lease")
            .field("name", &self.0.name)
            .field("holder", &self.0.holder)
            .field("config", &self.0.config)
            .finish_non_exhaustive()
    }
}

/// What an acquiring transaction decided, carried out of it.
enum Step {
    Held {
        epoch: i64,
        expires_at: DateTime<Utc>,
        /// `Some` when a tenure began: the gap since the lease was last held, if it ever was.
        began: Option<Option<TimeDelta>>,
    },
    Elsewhere {
        epoch: i64,
        expires_at: DateTime<Utc>,
    },
}

/// The database's time, read inside `tx` after its lock.
pub(crate) async fn cluster_now(
    tx: &mut SqlTransaction,
    lease: &str,
) -> Result<DateTime<Utc>, LeaseError> {
    match ClusterTime::read_in(tx).await {
        Ok(time) => Ok(time.to_utc()),
        Err(error) => {
            warn!(event = "lease.clock_unavailable", lease, %error, "cluster clock unreadable");
            Err(LeaseError::ClockUnavailable {
                lease: lease.to_owned(),
                cause: DatabaseError::from(error),
            })
        }
    }
}

fn expiry(lease: &str, now: DateTime<Utc>, ttl: TimeDelta) -> Result<DateTime<Utc>, LeaseError> {
    now.checked_add_signed(ttl)
        .ok_or_else(|| LeaseError::CorruptRow {
            lease: lease.to_owned(),
            detail: format!("{now} plus the TTL leaves the timestamp range"),
        })
}
