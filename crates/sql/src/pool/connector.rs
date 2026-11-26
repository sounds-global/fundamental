use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::Instant;

use arc_swap::ArcSwap;
use fundamental_core::AnyError;
use fundamental_db::{DatabaseError, DatabaseMetrics};
use parking_lot::Mutex;
use sqlx::pool::PoolConnection;
use sqlx::{PgPool, Postgres};
use tracing::warn;

use super::{AcquireGuard, SqlReservation};
use crate::{SqlConfig, SqlError, SqlResult};

/// What a refusal calls a pool no registry named.
const UNNAMED_POOL: &str = "unnamed";

/// A reservation of a pool: the reserved connector's pool, held weakly, and what it reserved.
type Reserved = (Weak<ArcSwap<PgPool>>, SqlReservation);

/// Reservations and the current configuration under one lock, so no reservation is opened on a credential a rotation passes by.
#[derive(Default)]
struct Reservations {
    config: Option<Arc<SqlConfig>>,
    entries: Vec<Reserved>,
}

/// A pool whose acquire this framework owns, so acquire time, timeouts and waiters can be observed.
#[derive(Clone)]
pub struct SqlConnector {
    /// Shared by every clone, so a credential rotation reaches them all at once.
    pool: Arc<ArcSwap<PgPool>>,
    /// Callers inside [`acquire`](Self::acquire) and not yet served, shared by every clone.
    waiters: Arc<AtomicU32>,
    metrics: Option<Arc<dyn DatabaseMetrics>>,
    /// What an operator calls this pool; `None` until a registry attaches it.
    label: Option<Arc<str>>,
    /// Weak, so an unheld reservation is not kept open; shared, as the rotated clone is not the one that reserved.
    reservations: Arc<Mutex<Reservations>>,
}

impl SqlConnector {
    /// Wraps `pool` so its acquisitions are counted and timed.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool: Arc::new(ArcSwap::from_pointee(pool)),
            waiters: Arc::new(AtomicU32::new(0)),
            metrics: None,
            label: None,
            reservations: Arc::new(Mutex::new(Reservations::default())),
        }
    }

    /// Reports acquisitions through `metrics`.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<dyn DatabaseMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Names this pool, so a refusal can say which one refused.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Takes a connection, counting this caller as waiting until it is served.
    ///
    /// # Errors
    /// Returns [`SqlError::Other`] carrying [`DatabaseError::PoolExhausted`] when the pool could
    /// not serve within its declared acquire bound. Any other driver failure keeps its own
    /// classification through [`SqlError::from`].
    pub async fn acquire(&self) -> SqlResult<PoolConnection<Postgres>> {
        // Dropped however the await ends, cancellation included.
        let waiting = AcquireGuard::held(&self.waiters);
        let started = Instant::now();
        let pool = self.pool.load_full();
        let acquired = pool.acquire().await;
        let waited = started.elapsed();
        drop(waiting);

        if let Some(metrics) = &self.metrics {
            metrics.record_connection_acquire(waited, acquired.is_ok());
        }
        acquired.map_err(|error| match error {
            sqlx::Error::PoolTimedOut => {
                let refusal = DatabaseError::pool_exhausted(
                    self.label.as_deref().unwrap_or(UNNAMED_POOL),
                    u64::try_from(waited.as_millis()).unwrap_or(u64::MAX),
                );
                // The pool's name and the wait, never the driver's sentence.
                warn!(
                    pool = self.label.as_deref().unwrap_or(UNNAMED_POOL),
                    waited_ms = waited.as_millis(),
                    waiting = self.waiting(),
                    "pool refused an acquire within its declared bound"
                );
                SqlError::Other(AnyError::new(refusal))
            }
            other => SqlError::from(other),
        })
    }

    /// How many callers are inside [`acquire`](Self::acquire) right now.
    #[must_use]
    pub fn waiting(&self) -> u32 {
        self.waiters.load(Ordering::Relaxed)
    }

    /// The pool serving now, for operations that are not a statement; never keep it, and never run a query on it.
    #[must_use]
    pub fn pool(&self) -> PgPool {
        PgPool::clone(&self.pool.load())
    }

    /// Puts `pool` behind this connector and every clone, returning the replaced pool to drain and close.
    pub(crate) fn replace_pool(&self, pool: PgPool) -> PgPool {
        PgPool::clone(&self.pool.swap(Arc::new(pool)))
    }

    /// Records `config` as the configuration this pool's connections are opened with now.
    pub(crate) fn with_config(self, config: Arc<SqlConfig>) -> Self {
        self.reservations.lock().config = Some(config);
        self
    }

    /// Opens a reservation with `open` from the current configuration and registers it under the rotation lock.
    pub(crate) fn reserve_with<T>(
        &self,
        fallback: &SqlConfig,
        reservation: &SqlReservation,
        open: impl FnOnce(&SqlConfig) -> SqlResult<(SqlConnector, T)>,
    ) -> SqlResult<(SqlConnector, T)> {
        let mut reservations = self.reservations.lock();
        let current = reservations.config.as_deref().unwrap_or(fallback);
        let (reserved, opened) = open(current)?;
        reservations
            .entries
            .retain(|(pool, _)| pool.strong_count() > 0);
        reservations
            .entries
            .push((Arc::downgrade(&reserved.pool), reservation.clone()));
        Ok((reserved, opened))
    }

    /// Records `fresh` as current and rebuilds every live reservation's pool from it, returning the replaced pools.
    pub(crate) fn replace_reservations(
        &self,
        fresh: Arc<SqlConfig>,
        mut rebuild: impl FnMut(&SqlConfig, &SqlReservation) -> SqlResult<PgPool>,
    ) -> (Vec<PgPool>, Vec<(SqlReservation, SqlError)>) {
        let mut reservations = self.reservations.lock();
        reservations
            .entries
            .retain(|(pool, _)| pool.strong_count() > 0);
        let mut replaced = Vec::with_capacity(reservations.entries.len());
        let mut failed = Vec::new();
        for (pool, reservation) in &reservations.entries {
            let Some(pool) = pool.upgrade() else { continue };
            match rebuild(&fresh, reservation) {
                Ok(rebuilt) => replaced.push(PgPool::clone(&pool.swap(Arc::new(rebuilt)))),
                Err(error) => failed.push((reservation.clone(), error)),
            }
        }
        reservations.config = Some(fresh);
        (replaced, failed)
    }
}

impl std::fmt::Debug for SqlConnector {
    /// Never prints the pool, whose connection options carry the credential.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SqlConnector")
            .field("waiting", &self.waiting())
            .field("has_metrics", &self.metrics.is_some())
            .field("reservations", &self.reservations.lock().entries.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use sqlx::postgres::PgPoolOptions;

    use super::*;

    /// Lazy, but sqlx still needs a Tokio runtime for the idle reaper.
    fn connector() -> SqlConnector {
        SqlConnector::new(
            PgPoolOptions::new()
                .connect_lazy("postgresql://nobody@127.0.0.1:1/none?sslmode=disable")
                .expect("a lazy pool needs no server"),
        )
    }

    #[tokio::test]
    async fn a_fresh_connector_has_nobody_waiting() {
        assert_eq!(connector().waiting(), 0);
    }

    #[tokio::test]
    async fn every_clone_counts_into_the_same_number() {
        let connector = connector();
        let clone = connector.clone();
        let _waiting = AcquireGuard::held(&connector.waiters);
        assert_eq!(clone.waiting(), 1, "a clone reports the same waiters");
    }

    #[tokio::test]
    async fn debug_never_prints_the_pool() {
        let rendered = format!("{:?}", connector());
        assert!(!rendered.contains("postgresql://"), "{rendered}");
        assert!(!rendered.contains("nobody"), "{rendered}");
    }
}
