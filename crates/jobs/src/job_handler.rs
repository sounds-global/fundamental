use std::future::Future;

use fundamental_core::AnyError;
use fundamental_db::IsolationLevel;
use fundamental_sql::SqlTransaction;

use crate::{Fire, FiringJob, JobKind};

/// What an application does when one of its jobs falls due.
///
/// [`fire`](Self::fire) runs inside the transaction that marks the job fired and may run again on a
/// retry, so an effect outside the database leaves through an outbox row written on `tx`. An
/// `Err`'s code is recorded on the job, so it must carry no personal data.
pub trait JobHandler<K: JobKind>: Send + Sync + 'static {
    /// The isolation the fire transaction runs at.
    ///
    /// Not `Serializable` by default: concurrent `Serializable` fires in different buckets deadlock
    /// one another on YugabyteDB.
    const ISOLATION: IsolationLevel = IsolationLevel::RepeatableRead;

    /// Performs `job`'s effect within `tx`, or decides it cannot be performed yet.
    ///
    /// # Errors
    ///
    /// Whatever stops the effect; the job stays pending and is retried after a backoff.
    fn fire(
        &self,
        tx: &mut SqlTransaction,
        job: &FiringJob<K>,
    ) -> impl Future<Output = Result<Fire, AnyError>> + Send;
}
