//! The commit-or-rollback decision, with the commit's own failure observable.

use tracing::{debug, warn};

use super::Transaction;
use crate::DatabaseError;
use fundamental_core::model::Database;

/// [`finalize`](super::finalize), with the commit's own failure handed to `observe` before `E` converts it.
///
/// A commit failure leaves no transaction to ask, and converting it to `E` may flatten its
/// SQLSTATE, so a retry loop sees it here. `observe` returns the error the caller receives,
/// so an adapter can also reclassify it.
///
/// # Errors
///
/// Returns `outcome`'s error, or a [`DatabaseError`] if the commit fails.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub async fn finalize_observed<Tx, D, T, E, O>(
    tx: Tx,
    outcome: Result<T, E>,
    observe: O,
) -> Result<T, E>
where
    Tx: Transaction<D>,
    D: Database,
    E: From<DatabaseError>,
    O: FnOnce(DatabaseError) -> DatabaseError,
{
    match outcome {
        Ok(value) => match tx.commit().await {
            Ok(()) => {
                debug!("transaction committed");
                Ok(value)
            }
            Err(error) => Err(E::from(observe(error))),
        },
        Err(error) => {
            if let Err(rollback_error) = tx.rollback().await {
                warn!(error = %rollback_error, "rollback after closure error failed");
            } else {
                debug!("transaction rolled back after closure error");
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use fundamental_core::Error as _;
    use fundamental_core::model::TestDb;

    use super::*;
    use crate::{
        DatabaseResult, IsolationLevel, TransactionOptions, TransactionScope, Transactional,
    };

    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<String>>>);

    impl Recorder {
        fn record(&self, event: &str) {
            self.0.lock().expect("recorder lock").push(event.to_owned());
        }

        fn events(&self) -> Vec<String> {
            self.0.lock().expect("recorder lock").clone()
        }
    }

    struct FakeTx {
        recorder: Recorder,
        fail_commit: bool,
    }

    impl Transaction<TestDb> for FakeTx {
        async fn commit(self) -> DatabaseResult<()> {
            self.recorder.record("commit");
            if self.fail_commit {
                return Err(DatabaseError::query("commit failed"));
            }
            Ok(())
        }

        async fn rollback(self) -> DatabaseResult<()> {
            self.recorder.record("rollback");
            Ok(())
        }
    }

    impl TransactionScope<TestDb> for FakeTx {
        async fn savepoint(&mut self, _name: &str) -> DatabaseResult<()> {
            Ok(())
        }

        async fn rollback_to_savepoint(&mut self, _name: &str) -> DatabaseResult<()> {
            Ok(())
        }

        async fn release_savepoint(&mut self, _name: &str) -> DatabaseResult<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakePool {
        recorder: Recorder,
        fail_commit: bool,
    }

    impl Transactional<TestDb> for FakePool {
        type Transaction = FakeTx;

        async fn begin_with(&self, _options: TransactionOptions) -> DatabaseResult<FakeTx> {
            self.recorder.record("begin");
            Ok(FakeTx {
                recorder: self.recorder.clone(),
                fail_commit: self.fail_commit,
            })
        }
    }

    #[tokio::test]
    async fn the_observer_is_shown_the_commits_own_failure() {
        // A commit failure is seen before `E::from` flattens it.
        let pool = FakePool {
            fail_commit: true,
            ..FakePool::default()
        };
        let tx = pool
            .begin(IsolationLevel::Serializable)
            .await
            .expect("begin");

        let mut seen = None;
        let result: Result<i32, DatabaseError> = finalize_observed(tx, Ok(1), |error| {
            seen = Some(error.error_code());
            error
        })
        .await;

        assert!(
            result.is_err(),
            "the commit failure still reaches the caller"
        );
        assert_eq!(
            seen,
            Some("DB_002"),
            "the observer sees the commit's own error, not the caller's"
        );
    }

    #[tokio::test]
    async fn the_observer_is_silent_when_the_closure_failed() {
        // A rollback is not a commit failure, so the observer is not called.
        let pool = FakePool::default();
        let tx = pool
            .begin(IsolationLevel::Serializable)
            .await
            .expect("begin");

        let mut seen = None;
        let result: Result<i32, DatabaseError> =
            finalize_observed(tx, Err(DatabaseError::query("boom")), |error| {
                seen = Some(error.error_code());
                error
            })
            .await;

        assert!(result.is_err());
        assert_eq!(seen, None, "nothing was committed, so nothing is observed");
        assert_eq!(pool.recorder.events(), ["begin", "rollback"]);
    }

    #[tokio::test]
    async fn the_observer_can_reclassify_the_commits_failure() {
        let pool = FakePool {
            fail_commit: true,
            ..FakePool::default()
        };
        let tx = pool
            .begin(IsolationLevel::Serializable)
            .await
            .expect("begin");

        let result: Result<i32, DatabaseError> = finalize_observed(tx, Ok(1), |error| {
            DatabaseError::commit_outcome_unknown(error.to_string())
        })
        .await;

        let error = result.expect_err("the commit failed");
        assert_eq!(
            error.error_code(),
            "DB_012",
            "the caller receives what the observer returned"
        );
    }
}
