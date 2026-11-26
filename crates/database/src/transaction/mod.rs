//! Transaction management traits. The `pool` and models below come from a backend crate:
//!
//! ```text
//! // Using the transaction helper: the async closure borrows `&mut tx`, and the
//! // helper commits automatically when it returns `Ok`, or rolls back on `Err`.
//! pool.transaction(IsolationLevel::Serializable, async |tx| {
//!     tx.insert(&new_user).await?;
//!     tx.insert(&new_order).await?;
//!     Ok(())
//! }).await?;
//!
//! // Manual transaction management. The level is an argument here too — there is no
//! // form that inherits the server's default, because what that default is depends on
//! // the engine and is written down nowhere the caller can see.
//! let tx = pool.begin(IsolationLevel::Serializable).await?;
//! // ... operations against `&mut tx` ...
//! tx.commit().await?;
//! ```

mod effect_model;
mod finalize_observed;
mod guard;
mod isolation_level;
mod keyed_repository_transaction;
mod keyed_repository_transaction_batch;
mod options;
mod repository_transaction;
mod repository_transaction_aggregate;
mod repository_transaction_batch;
mod repository_transaction_count;
mod repository_transaction_ext;
mod repository_transaction_query;
mod repository_transaction_soft_delete;
mod repository_transaction_translation;
mod transaction_scope;
mod transactional;

pub use effect_model::EffectModel;
pub use finalize_observed::finalize_observed;
pub use guard::TransactionGuard;
pub use isolation_level::IsolationLevel;
pub use keyed_repository_transaction::KeyedRepositoryTransaction;
pub use keyed_repository_transaction_batch::KeyedRepositoryTransactionBatch;
pub use options::TransactionOptions;
pub use repository_transaction::RepositoryTransaction;
pub use repository_transaction_aggregate::RepositoryTransactionAggregate;
pub use repository_transaction_batch::RepositoryTransactionBatch;
pub use repository_transaction_count::RepositoryTransactionCount;
pub use repository_transaction_ext::RepositoryTransactionExt;
pub use repository_transaction_query::RepositoryTransactionQuery;
pub use repository_transaction_soft_delete::RepositoryTransactionSoftDelete;
pub use repository_transaction_translation::RepositoryTransactionTranslation;
pub use transaction_scope::TransactionScope;
pub use transactional::Transactional;

use super::{DatabaseError, DatabaseResult};
use fundamental_core::model::Database;

/// A database transaction.
///
/// Rolled back when dropped without being committed. Typed CRUD inside it is through
/// [`RepositoryTransaction`] and [`RepositoryTransactionBatch`].
pub trait Transaction<D>: TransactionScope<D>
where
    D: Database,
{
    /// Commits the transaction.
    fn commit(self) -> impl Future<Output = DatabaseResult<()>> + Send;

    /// Rolls back the transaction.
    fn rollback(self) -> impl Future<Output = DatabaseResult<()>> + Send;
}

// No closure helper here: retrying needs a sleep and this crate is runtime-neutral. The
// retrying form is `SqlPool::transaction`.

/// Commits `tx` on `Ok`, rolls it back on `Err`, and returns the outcome.
///
/// A rollback failure after an error is logged and the original error wins. Pair it with
/// [`Transactional::begin`] in a caller's own retry loop.
///
/// # Errors
///
/// Returns `outcome`'s error, or a [`DatabaseError`] if the commit fails.
pub async fn finalize<Tx, D, T, E>(tx: Tx, outcome: Result<T, E>) -> Result<T, E>
where
    Tx: Transaction<D>,
    D: Database,
    E: From<DatabaseError>,
{
    finalize_observed(tx, outcome, |error| error).await
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use fundamental_core::Error as _;
    use fundamental_core::model::TestDb;
    use tracing_test::traced_test;

    use super::*;
    use crate::DatabaseError;

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
        fail_rollback: bool,
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
            if self.fail_rollback {
                return Err(DatabaseError::query("rollback failed"));
            }
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
        fail_rollback: bool,
        begun_isolation: Arc<Mutex<Option<IsolationLevel>>>,
    }

    impl Transactional<TestDb> for FakePool {
        type Transaction = FakeTx;

        async fn begin_with(&self, options: TransactionOptions) -> DatabaseResult<FakeTx> {
            *self.begun_isolation.lock().expect("isolation lock") = Some(options.isolation_level);
            self.recorder.record("begin");
            Ok(FakeTx {
                recorder: self.recorder.clone(),
                fail_commit: self.fail_commit,
                fail_rollback: self.fail_rollback,
            })
        }
    }

    #[test]
    fn test_isolation_level_sql() {
        assert_eq!(IsolationLevel::Serializable.as_sql(), "SERIALIZABLE");
        assert_eq!(IsolationLevel::ReadCommitted.as_sql(), "READ COMMITTED");
    }

    #[test]
    fn test_transaction_options() {
        let opts = TransactionOptions::new(IsolationLevel::Serializable)
            .isolation_level(IsolationLevel::ReadCommitted)
            .read_only()
            .timeout(5000);

        assert_eq!(opts.isolation_level, IsolationLevel::ReadCommitted);
        assert!(opts.read_only);
        assert_eq!(opts.timeout_ms, Some(5000));
    }

    /// `new` takes the level, so nothing constructs options without stating one.
    #[test]
    fn options_carry_the_level_they_were_built_with() {
        let opts = TransactionOptions::new(IsolationLevel::RepeatableRead);

        assert_eq!(opts.isolation_level, IsolationLevel::RepeatableRead);
        assert!(!opts.read_only, "read-write unless asked otherwise");
        assert!(!opts.deferrable);
        assert_eq!(opts.timeout_ms, None);
    }

    #[tokio::test]
    async fn helper_commits_on_ok() {
        let pool = FakePool::default();
        let tx = pool
            .begin(IsolationLevel::Serializable)
            .await
            .expect("begin");
        let result: Result<i32, DatabaseError> = finalize(tx, Ok(42)).await;

        assert_eq!(result.expect("closure ok"), 42);
        assert_eq!(pool.recorder.events(), ["begin", "commit"]);
    }

    #[tokio::test]
    async fn helper_rolls_back_on_err() {
        let pool = FakePool::default();
        let tx = pool
            .begin(IsolationLevel::Serializable)
            .await
            .expect("begin");
        let result: Result<i32, DatabaseError> =
            finalize(tx, Err(DatabaseError::query("boom"))).await;

        let err = result.expect_err("closure error propagates");
        assert!(matches!(err, DatabaseError::Query { .. }));
        assert_eq!(err.error_code(), "DB_002");
        assert_eq!(pool.recorder.events(), ["begin", "rollback"]);
    }

    #[tokio::test]
    async fn helper_returns_commit_error() {
        let pool = FakePool {
            fail_commit: true,
            ..FakePool::default()
        };
        let tx = pool
            .begin(IsolationLevel::Serializable)
            .await
            .expect("begin");
        let result: Result<i32, DatabaseError> = finalize(tx, Ok(1)).await;

        let err = result.expect_err("commit failure surfaces");
        assert_eq!(err.error_code(), "DB_002");
        assert_eq!(pool.recorder.events(), ["begin", "commit"]);
    }

    #[tokio::test]
    #[traced_test]
    async fn helper_logs_when_rollback_after_error_fails() {
        let pool = FakePool {
            fail_rollback: true,
            ..FakePool::default()
        };
        let tx = pool
            .begin(IsolationLevel::Serializable)
            .await
            .expect("begin");
        let result: Result<i32, DatabaseError> =
            finalize(tx, Err(DatabaseError::query("boom"))).await;

        // The original closure error wins over the rollback failure.
        let err = result.expect_err("closure error propagates");
        assert!(err.to_string().contains("boom") || err.error_code() == "DB_002");
        assert_eq!(pool.recorder.events(), ["begin", "rollback"]);
        assert!(logs_contain("rollback after closure error failed"));
    }

    #[tokio::test]
    async fn helper_passes_isolation_level_through() {
        let pool = FakePool::default();
        let tx = pool
            .begin(IsolationLevel::ReadCommitted)
            .await
            .expect("begin");
        let _: Result<(), DatabaseError> = finalize(tx, Ok(())).await;

        assert_eq!(
            *pool.begun_isolation.lock().expect("isolation lock"),
            Some(IsolationLevel::ReadCommitted)
        );
        assert_eq!(pool.recorder.events(), ["begin", "commit"]);
    }
}
