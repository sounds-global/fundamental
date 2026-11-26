//! Work writes only through the fence. Requires Docker.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use fundamental_core::Error as _;
use fundamental_db::{DatabaseError, IsolationLevel, RepositoryTransaction as _};
use fundamental_lease::{Acquisition, Fenced, HeldLease, LeaseError, WorkKey};
use fundamental_macros::{Error, Model};
use fundamental_sql::{SqlPool, SqlTransaction};
use sqlx::Row as _;
use support::{PAST_TTL, lease, schema_with_leases};
use tokio::sync::oneshot;

/// What the fenced work writes, so a test can count committed effects.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "fenced_effects")]
struct FencedEffect {
    #[model(id)]
    id: String,
    epoch: i64,
}

/// A caller's error type converting from both the lease's refusals and the store's.
#[derive(Debug, Error)]
enum WorkError {
    #[error(transparent)]
    Lease(#[from] LeaseError),
    #[error(transparent)]
    Store(#[from] DatabaseError),
}

async fn effects_table(pool: &SqlPool) {
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS fenced_effects (id TEXT NOT NULL, epoch BIGINT NOT NULL, \
         PRIMARY KEY (id HASH))",
    )
    .await
    .expect("the effects table");
}

async fn count_effects(pool: &SqlPool) -> i64 {
    let row = pool
        .fetch_one("SELECT count(*) FROM fenced_effects")
        .await
        .expect("a count");
    row.try_get(0).expect("an integer")
}

fn held(acquisition: Acquisition) -> HeldLease {
    match acquisition {
        Acquisition::Held(proof) => proof,
        Acquisition::HeldElsewhere { epoch, .. } => panic!("held elsewhere at epoch {epoch}"),
    }
}

fn key(text: &str) -> WorkKey {
    WorkKey::new(text).expect("a work key")
}

async fn write_effect(
    proof: &HeldLease,
    work: &WorkKey,
    id: &str,
) -> Result<Fenced<()>, WorkError> {
    let id = id.to_owned();
    let epoch = proof.epoch();
    proof
        .transaction(
            work,
            IsolationLevel::Serializable,
            async move |tx: &mut SqlTransaction| {
                tx.insert(&FencedEffect { id, epoch }).await?;
                Ok::<_, WorkError>(())
            },
        )
        .await
}

#[tokio::test]
async fn a_fenced_write_commits_while_the_lease_is_held() {
    let schema = schema_with_leases().await;
    effects_table(schema.pool()).await;
    let proof = held(
        lease(schema.pool(), "keeper")
            .try_acquire()
            .await
            .expect("an answer"),
    );
    let outcome = write_effect(&proof, &key("tick:1"), "a")
        .await
        .expect("committed");
    assert_eq!(outcome, Fenced::Done(()));
    assert_eq!(count_effects(schema.pool()).await, 1);
}

#[tokio::test]
async fn a_paused_holders_fenced_write_is_refused_after_a_takeover() {
    let schema = schema_with_leases().await;
    effects_table(schema.pool()).await;
    let alpha = lease(schema.pool(), "keeper");
    let paused = held(alpha.try_acquire().await.expect("an answer"));
    tokio::time::sleep(PAST_TTL).await;
    held(
        lease(schema.pool(), "keeper")
            .try_acquire()
            .await
            .expect("the takeover"),
    );

    let error = write_effect(&paused, &key("tick:1"), "late")
        .await
        .expect_err("a superseded tenure cannot write");
    assert_eq!(error.error_code(), "SLEASE_002", "{error:?}");
    assert_eq!(count_effects(schema.pool()).await, 0, "nothing was written");
}

#[tokio::test]
async fn a_proof_from_a_lost_tenure_cannot_fence() {
    let schema = schema_with_leases().await;
    effects_table(schema.pool()).await;
    let alpha = lease(schema.pool(), "keeper");
    let old = held(alpha.try_acquire().await.expect("an answer"));
    alpha.release().await.expect("released");
    let new = held(alpha.try_acquire().await.expect("a new tenure"));
    assert!(new.epoch() > old.epoch());

    let error = write_effect(&old, &key("tick:1"), "stale")
        .await
        .expect_err("the old proof is refused");
    assert_eq!(error.error_code(), "SLEASE_002");
    write_effect(&new, &key("tick:1"), "fresh")
        .await
        .expect("the new proof writes");
    assert_eq!(count_effects(schema.pool()).await, 1);
}

#[tokio::test]
async fn a_holder_that_let_its_lease_expire_begins_a_new_tenure() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    let first = held(alpha.try_acquire().await.expect("an answer"));
    tokio::time::sleep(PAST_TTL).await;
    let second = held(alpha.try_acquire().await.expect("an answer"));
    assert!(
        second.began(),
        "an expired tenure is over, even for its own holder"
    );
    assert!(second.epoch() > first.epoch());
}

#[tokio::test]
async fn work_already_committed_is_not_run_again() {
    let schema = schema_with_leases().await;
    effects_table(schema.pool()).await;
    let proof = held(
        lease(schema.pool(), "keeper")
            .try_acquire()
            .await
            .expect("an answer"),
    );
    let job = key("due:job-1");
    assert_eq!(
        write_effect(&proof, &job, "a").await.expect("first"),
        Fenced::Done(())
    );
    assert_eq!(
        write_effect(&proof, &job, "b").await.expect("second"),
        Fenced::AlreadyDone
    );
    assert_eq!(count_effects(schema.pool()).await, 1);
}

#[tokio::test]
async fn a_fenced_body_whose_commit_was_lost_does_not_run_twice() {
    let schema = schema_with_leases().await;
    effects_table(schema.pool()).await;
    let pool = schema.pool().clone();
    let proof = held(
        lease(&pool, "keeper")
            .try_acquire()
            .await
            .expect("an answer"),
    );
    let job = key("due:job-2");
    let runs = Arc::new(AtomicU32::new(0));

    // The connection is terminated before COMMIT, so the answer is lost.
    let outcome = proof
        .transaction(&job, IsolationLevel::Serializable, {
            let (runs, pool) = (Arc::clone(&runs), pool.clone());
            let epoch = proof.epoch();
            async move |tx: &mut SqlTransaction| {
                runs.fetch_add(1, Ordering::SeqCst);
                tx.insert(&FencedEffect {
                    id: "once".to_owned(),
                    epoch,
                })
                .await?;
                let row = tx
                    .fetch_one("SELECT pg_backend_pid()")
                    .await
                    .map_err(DatabaseError::from)?;
                let pid: i32 = row
                    .try_get(0)
                    .map_err(|e| DatabaseError::internal(e.to_string()))?;
                pool.execute(&format!("SELECT pg_terminate_backend({pid})"))
                    .await
                    .map_err(DatabaseError::from)?;
                Ok::<_, WorkError>(())
            }
        })
        .await;
    let error = outcome.expect_err("the commit's answer was lost");
    assert_eq!(error.error_code(), "DB_012", "{error:?}");
    assert_eq!(runs.load(Ordering::SeqCst), 1, "the body ran once");

    // Asking again with the same key finds out; the effect exists exactly once.
    let again = write_effect(&proof, &job, "once").await.expect("an answer");
    let third = write_effect(&proof, &job, "once").await.expect("an answer");
    assert_eq!(third, Fenced::AlreadyDone);
    assert!(matches!(again, Fenced::Done(()) | Fenced::AlreadyDone));
    assert_eq!(count_effects(&pool).await, 1);
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
async fn a_takeover_waits_for_a_fenced_transaction_in_flight() {
    let schema = schema_with_leases().await;
    effects_table(schema.pool()).await;
    let pool = schema.pool().clone();
    let alpha = lease(&pool, "keeper");
    let proof = held(alpha.try_acquire().await.expect("an answer"));
    let (entered_tx, entered) = oneshot::channel::<()>();
    let (release_tx, release) = oneshot::channel::<()>();
    let entered_tx = Arc::new(std::sync::Mutex::new(Some(entered_tx)));
    let release = Arc::new(tokio::sync::Mutex::new(Some(release)));

    let writer = tokio::spawn({
        let proof = proof.clone();
        let epoch = proof.epoch();
        async move {
            proof
                .transaction(
                    &key("tick:held"),
                    IsolationLevel::Serializable,
                    async move |tx: &mut SqlTransaction| {
                        tx.insert(&FencedEffect {
                            id: "in-flight".to_owned(),
                            epoch,
                        })
                        .await?;
                        if let Some(signal) =
                            entered_tx.lock().ok().and_then(|mut slot| slot.take())
                        {
                            let _ = signal.send(());
                        }
                        if let Some(wait) = release.lock().await.take() {
                            let _ = wait.await;
                        }
                        Ok::<_, WorkError>(())
                    },
                )
                .await
        }
    });
    entered
        .await
        .expect("the fenced transaction is holding the row");
    // Past the TTL by the database's clock, the fenced transaction still holds the row lock.
    tokio::time::sleep(PAST_TTL).await;
    let challenger = tokio::spawn({
        let beta = lease(&pool, "keeper");
        async move { beta.try_acquire().await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !challenger.is_finished(),
        "the takeover must wait for the fenced transaction"
    );
    release_tx.send(()).expect("the writer is waiting");
    writer
        .await
        .expect("the writer task")
        .expect("the in-flight write commits");
    let taken = held(
        challenger
            .await
            .expect("the challenger task")
            .expect("an answer"),
    );
    assert!(taken.epoch() > proof.epoch());
    assert_eq!(count_effects(&pool).await, 1);
}

#[tokio::test]
async fn work_older_than_the_retention_window_runs_again() {
    let schema = schema_with_leases().await;
    effects_table(schema.pool()).await;
    let proof = held(
        lease(schema.pool(), "keeper")
            .try_acquire()
            .await
            .expect("an answer"),
    );
    let job = key("due:job-3");
    assert_eq!(
        write_effect(&proof, &job, "first").await.expect("first"),
        Fenced::Done(())
    );
    let far_future = proof.expires_at() + chrono::TimeDelta::days(3_650);
    assert_eq!(
        proof.forget_work_before(far_future).await.expect("pruned"),
        1
    );
    assert_eq!(
        write_effect(&proof, &job, "second").await.expect("again"),
        Fenced::Done(()),
        "past its retention window a key is forgotten, and the work runs again"
    );
    assert_eq!(count_effects(schema.pool()).await, 2);
}
