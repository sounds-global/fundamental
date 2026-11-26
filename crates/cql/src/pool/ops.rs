//! The [`OperationGuard`] for operation-duration leak detection.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use tracing::warn;

/// Guard for tracking operation duration.
///
/// # Example
///
/// ```rust,no_run
/// use fundamental_db::Repository;
/// use fundamental_cql::{CqlConfig, CqlPool};
/// # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
/// # #[model(cql, table = "events")]
/// # struct Event {
/// #     #[model(partition_key)]
/// #     tenant_id: uuid::Uuid,
/// #     payload: String,
/// # }
/// # async fn example(config: CqlConfig, event: Event) -> Result<(), Box<dyn std::error::Error>> {
/// let pool = CqlPool::connect(&config).await?;
/// let guard = pool.operation_guard();
///
/// // Do work with session
/// pool.repository::<Event>().insert(&event).await?;
///
/// // Guard is dropped here, checking operation duration
/// drop(guard);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct OperationGuard {
    started_at: Instant,
    threshold: Option<Duration>,
    leak_warnings: Arc<AtomicU64>,
    operation: Option<String>,
    in_flight: Option<Arc<AtomicU64>>,
}

impl OperationGuard {
    /// Creates a new operation guard.
    pub fn new(threshold: Option<Duration>, leak_warnings: Arc<AtomicU64>) -> Self {
        Self {
            started_at: Instant::now(),
            threshold,
            leak_warnings,
            operation: None,
            in_flight: None,
        }
    }

    /// Creates a new operation guard with an operation name for better logging.
    pub fn with_operation(
        threshold: Option<Duration>,
        leak_warnings: Arc<AtomicU64>,
        operation: impl Into<String>,
    ) -> Self {
        Self {
            started_at: Instant::now(),
            threshold,
            leak_warnings,
            operation: Some(operation.into()),
            in_flight: None,
        }
    }

    /// Creates a new operation guard with in-flight tracking.
    pub(crate) fn with_in_flight(
        threshold: Option<Duration>,
        leak_warnings: Arc<AtomicU64>,
        in_flight: Arc<AtomicU64>,
    ) -> Self {
        in_flight.fetch_add(1, Ordering::Relaxed);
        Self {
            started_at: Instant::now(),
            threshold,
            leak_warnings,
            operation: None,
            in_flight: Some(in_flight),
        }
    }

    /// Creates a new operation guard with in-flight tracking and an operation name.
    pub(crate) fn with_in_flight_named(
        threshold: Option<Duration>,
        leak_warnings: Arc<AtomicU64>,
        in_flight: Arc<AtomicU64>,
        operation: impl Into<String>,
    ) -> Self {
        in_flight.fetch_add(1, Ordering::Relaxed);
        Self {
            started_at: Instant::now(),
            threshold,
            leak_warnings,
            operation: Some(operation.into()),
            in_flight: Some(in_flight),
        }
    }

    /// Returns how long the operation has been running.
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }
}

impl Drop for OperationGuard {
    #[expect(
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn drop(&mut self) {
        if let Some(ref in_flight) = self.in_flight {
            in_flight.fetch_sub(1, Ordering::Relaxed);
        }

        if let Some(threshold) = self.threshold {
            let elapsed = self.started_at.elapsed();
            if elapsed > threshold {
                if let Some(ref op) = self.operation {
                    warn!(
                        operation = %op,
                        elapsed_ms = elapsed.as_millis(),
                        threshold_ms = threshold.as_millis(),
                        "Operation exceeded leak detection threshold"
                    );
                } else {
                    warn!(
                        elapsed_ms = elapsed.as_millis(),
                        threshold_ms = threshold.as_millis(),
                        "Operation exceeded leak detection threshold"
                    );
                }
                self.leak_warnings.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
