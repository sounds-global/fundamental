use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use fundamental_db::{
    DatabaseError, Filter, FilterValue, IsolationLevel, Repository as _,
    RepositoryTransaction as _, RepositoryTransactionQuery as _,
};
use fundamental_sql::SqlTransaction;
use tokio::sync::watch;
use tokio::time::{Instant, sleep_until};
use tracing::{debug, instrument, warn};

use crate::lease::cluster_now;
use crate::{Fenced, Lease, LeaseError, LeaseName, LeaseRow, LeaseState, LeaseWorkRow, WorkKey};

/// Proof that this process holds a lease at an epoch — the only way to write under it.
///
/// [`transaction`](Self::transaction) checks it again under the row's lock, since a tenure can end while held.
#[derive(Clone)]
pub struct HeldLease {
    lease: Lease,
    epoch: i64,
    expires_at: DateTime<Utc>,
    deadline: Instant,
    began: Option<Option<Duration>>,
    watch: Option<watch::Receiver<Option<HeldLease>>>,
}

impl HeldLease {
    pub(crate) fn new(
        lease: Lease,
        epoch: i64,
        expires_at: DateTime<Utc>,
        deadline: Instant,
        began: Option<Option<Duration>>,
    ) -> Self {
        Self {
            lease,
            epoch,
            expires_at,
            deadline,
            began,
            watch: None,
        }
    }

    /// This proof, told of the surface's later renewals and losses through `watch`.
    pub(crate) fn watched(mut self, watch: watch::Receiver<Option<HeldLease>>) -> Self {
        self.watch = Some(watch);
        self
    }

    /// Which lease.
    #[must_use]
    pub fn name(&self) -> LeaseName {
        self.lease.name()
    }

    /// The tenure's fencing epoch.
    #[must_use]
    pub fn epoch(&self) -> i64 {
        self.epoch
    }

    /// When, by the database's clock, the lease expires unless renewed.
    #[must_use]
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    /// Whether this acquisition began the tenure rather than renewing it.
    #[must_use]
    pub fn began(&self) -> bool {
        self.began.is_some()
    }

    /// For a tenure this acquisition began, how long the lease stood unheld before it.
    #[must_use]
    pub fn handover_gap(&self) -> Option<Duration> {
        self.began.flatten()
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Resolves once this tenure is over as far as this process can tell.
    pub async fn lost(&self) {
        let Some(watch) = &self.watch else {
            sleep_until(self.deadline).await;
            return;
        };
        let mut watch = watch.clone();
        loop {
            let deadline = match watch.borrow_and_update().as_ref() {
                Some(current) if current.epoch == self.epoch => current.deadline,
                _ => return,
            };
            tokio::select! {
                biased;
                () = sleep_until(deadline) => return,
                changed = watch.changed() => if changed.is_err() { return },
            }
        }
    }

    /// Runs `work` in one transaction that commits only while this tenure still holds the
    /// lease, and at most once for `key`.
    ///
    /// Its first statement locks the lease row and requires this holder, epoch and an unexpired lease.
    ///
    /// # Errors
    ///
    /// `E` from [`LeaseError::Superseded`] when the tenure is over, from another [`LeaseError`],
    /// `work`'s own error, or a [`DatabaseError`]; after [`DatabaseError::CommitOutcomeUnknown`]
    /// call again with the same key.
    #[instrument(name = "lease.fence", skip(self, work), fields(lease = %self.name(), epoch = self.epoch, key = %key), err(Debug))]
    pub async fn transaction<F, T, E>(
        &self,
        key: &WorkKey,
        level: IsolationLevel,
        work: F,
    ) -> Result<Fenced<T>, E>
    where
        F: AsyncFnOnce(&mut SqlTransaction) -> Result<T, E> + Clone,
        E: From<LeaseError> + From<DatabaseError> + fundamental_core::Error,
    {
        let name = self.name().as_str();
        let me = self.lease.holder().as_uuid();
        let epoch = self.epoch;
        let key = key.as_str();
        let started = Instant::now();
        let outcome = self
            .lease
            .pool()
            .transaction(level, async move |tx| {
                let now = fence(tx, name, me, epoch).await.map_err(E::from)?;
                let record = LeaseWorkRow::new(name, key, epoch, now);
                if tx.insert_if_absent(&record).await?.is_none() {
                    return Ok(Fenced::AlreadyDone);
                }
                work(tx).await.map(Fenced::Done)
            })
            .await;
        let elapsed = started.elapsed();
        if elapsed > self.lease.config().renew_every() {
            warn!(
                event = "lease.fenced_transaction_slow",
                lease = name,
                elapsed_ms = elapsed.as_millis(),
                "a fenced transaction held the lease row longer than a renewal period"
            );
        }
        match &outcome {
            Ok(Fenced::AlreadyDone) => {
                debug!(lease = name, key, "work already committed; not run again")
            }
            Ok(Fenced::Done(_)) => debug!(lease = name, key, "fenced work committed"),
            Err(error) => warn!(
                event = "lease.fence_refused",
                lease = name,
                epoch,
                key,
                code = error.error_code(),
                "fenced work did not commit"
            ),
        }
        outcome
    }
}

impl HeldLease {
    /// Forgets committed work keys recorded before `cutoff`, returning how many.
    ///
    /// # Errors
    ///
    /// [`LeaseError::Unavailable`] or [`LeaseError::TableMissing`].
    #[instrument(name = "lease.forget_work", skip(self), fields(lease = %self.name()), err(Debug))]
    pub async fn forget_work_before(&self, cutoff: DateTime<Utc>) -> Result<u64, LeaseError> {
        let name = self.name().as_str();
        let query = self
            .lease
            .pool()
            .repository::<LeaseWorkRow>()
            .query()
            .pin_key(name)
            .filter(Filter::lt(
                LeaseWorkRow::DONE_AT,
                FilterValue::TimestampTz(cutoff),
            ));
        self.lease
            .pool()
            .transaction(IsolationLevel::ReadCommitted, async |tx| {
                Ok::<_, LeaseError>(tx.delete_where(&query).await?)
            })
            .await
            .map_err(|error| error.about(name))
    }
}

/// Locks the lease row and checks it still carries `me` at `epoch`, unexpired.
async fn fence(
    tx: &mut SqlTransaction,
    name: &str,
    me: uuid::Uuid,
    epoch: i64,
) -> Result<DateTime<Utc>, LeaseError> {
    let row = tx
        .find_for_update::<LeaseRow>(&name.to_owned())
        .await
        .map_err(|error| LeaseError::Store(error).about(name))?;
    let now = cluster_now(tx, name).await?;
    match row.as_ref().map(LeaseRow::state).transpose()? {
        Some(LeaseState::Held {
            holder,
            epoch: held,
            expires_at,
            ..
        }) if holder == me && held == epoch && expires_at > now => Ok(now),
        _ => Err(LeaseError::Superseded {
            lease: name.to_owned(),
            epoch,
        }),
    }
}

impl fmt::Debug for HeldLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeldLease")
            .field("lease", &self.lease.name())
            .field("epoch", &self.epoch)
            .field("expires_at", &self.expires_at)
            .field("began", &self.began())
            .finish_non_exhaustive()
    }
}
