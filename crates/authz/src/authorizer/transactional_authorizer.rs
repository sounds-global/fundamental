//! [`TransactionalAuthorizer`]: an authorization decision taken inside the caller's
//! transaction.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use fundamental_db::{DatabaseError, IsolationLevel};
use fundamental_sql::{ClusterTime, SqlTransaction};
use tracing::field::Empty;
use tracing::{Instrument, Span, info_span, instrument, warn};

use super::{DynAuthorizer, LockedDecision, LockedQuestions, NativeEngine};
use crate::backend::{SqlTransactionView, SqlTupleStore};
use crate::engine::{Engine, EvalLimits, UsersetTree, Witness};
use crate::error::AuthzError;
use crate::schema::RelationName;
use crate::tuple::{CaveatContext, ObjectRef, SubjectRef, WallTime};

type DecisionFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, AuthzError>> + Send + 'a>>;

/// An authorizer that answers a relation question **inside** a transaction the caller
/// holds, so the decision and the write it guards observe one state.
///
/// The read half of [`SqlTupleStore::write_in`] and [`SqlTupleStore::delete_in`]: a
/// read-decide-write in one snapshot. It refuses a transaction weaker than `SERIALIZABLE`
/// ([`AuthzError::IsolationTooWeak`]), never consults the set index, and returns no zookie.
/// Implemented for `NativeEngine<SqlTupleStore>` alone.
pub trait TransactionalAuthorizer: DynAuthorizer {
    /// Whether `subject` holds `relation` on `object`, read through `tx`.
    ///
    /// # Errors
    ///
    /// [`AuthzError::IsolationTooWeak`] when `tx` was not begun `SERIALIZABLE`;
    /// [`AuthzError::Storage`] when the cluster's clock cannot be read in `tx`; otherwise the
    /// errors of [`Engine::check_in`].
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    fn check_in<'a>(
        &'a self,
        tx: &'a mut SqlTransaction,
        object: ObjectRef,
        relation: RelationName,
        subject: SubjectRef,
        context: CaveatContext,
    ) -> DecisionFuture<'a, bool>;

    /// Whether each of `questions` holds, decided inside the act's own `READ COMMITTED`
    /// transaction so that nothing the allows rest on can be revoked before `tx` ends.
    ///
    /// The witnesses of the allowed questions are locked `FOR UPDATE` in ascending key order, then
    /// re-checked at the clock read after the last lock, so nothing an allow rests on can be
    /// revoked before `tx` ends. Sound because a locked relation reaches no exclusion. A moved
    /// witness tuple is [`AuthzError::LockedWitnessMoved`], which is retryable.
    ///
    /// # Errors
    ///
    /// [`AuthzError::LockedCheckIsolation`] when `tx` was not begun `READ COMMITTED`;
    /// [`AuthzError::LockedCheckInSavepoint`] while `tx` holds a savepoint;
    /// [`AuthzError::RelationNotLocked`]; [`AuthzError::ExclusionInLockedWalk`];
    /// [`AuthzError::LockBudgetExceeded`] past [`LockedQuestions::WITNESS_LIMIT`];
    /// [`AuthzError::LockedWitnessMoved`]; otherwise the errors of [`check_in`](Self::check_in).
    fn check_in_locked<'a>(
        &'a self,
        tx: &'a mut SqlTransaction,
        questions: LockedQuestions,
    ) -> DecisionFuture<'a, LockedDecision>;

    /// The subject tree holding `relation` on `object`, read through `tx`.
    ///
    /// # Errors
    ///
    /// As [`check_in`](Self::check_in), and the errors of [`Engine::expand_in`].
    fn expand_in<'a>(
        &'a self,
        tx: &'a mut SqlTransaction,
        object: ObjectRef,
        relation: RelationName,
    ) -> DecisionFuture<'a, UsersetTree>;
}

impl TransactionalAuthorizer for NativeEngine<SqlTupleStore> {
    fn check_in<'a>(
        &'a self,
        tx: &'a mut SqlTransaction,
        object: ObjectRef,
        relation: RelationName,
        subject: SubjectRef,
        context: CaveatContext,
    ) -> DecisionFuture<'a, bool> {
        Box::pin(check_within(self, tx, object, relation, subject, context))
    }

    fn check_in_locked<'a>(
        &'a self,
        tx: &'a mut SqlTransaction,
        questions: LockedQuestions,
    ) -> DecisionFuture<'a, LockedDecision> {
        Box::pin(locked_within(self, tx, questions))
    }

    fn expand_in<'a>(
        &'a self,
        tx: &'a mut SqlTransaction,
        object: ObjectRef,
        relation: RelationName,
    ) -> DecisionFuture<'a, UsersetTree> {
        Box::pin(expand_within(self, tx, object, relation))
    }
}

#[instrument(
    name = "authz.check_in_locked",
    skip_all,
    fields(questions = questions.len(), allowed = Empty, witness_tuples = Empty, walk_reads = Empty, lock_ms = Empty, outcome = Empty),
    err(level = "warn", Debug)
)]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn locked_within(
    authorizer: &NativeEngine<SqlTupleStore>,
    tx: &mut SqlTransaction,
    questions: LockedQuestions,
) -> Result<LockedDecision, AuthzError> {
    require_read_committed(tx)?;
    let open = tx.open_savepoints();
    if open > 0 {
        warn!(
            open,
            "locked check refused: a savepoint is open, and rolling back to it would release the locks"
        );
        return Err(AuthzError::LockedCheckInSavepoint { open });
    }

    let (answers, witness) = walk_locked(authorizer, tx, &questions).await?;
    let span = Span::current();
    span.record(
        "allowed",
        answers.iter().filter(|allowed| **allowed).count(),
    );
    span.record("witness_tuples", witness.len());
    if witness.len() > LockedQuestions::WITNESS_LIMIT {
        warn!(
            tuples = witness.len(),
            limit = LockedQuestions::WITNESS_LIMIT,
            "locked check refused: the witness is larger than one act may lock"
        );
        return Err(AuthzError::LockBudgetExceeded {
            tuples: witness.len(),
            limit: LockedQuestions::WITNESS_LIMIT,
        });
    }
    if witness.len() == 0 {
        let outcome = if answers.iter().any(|allowed| *allowed) {
            "allowed"
        } else {
            "denied"
        };
        span.record("outcome", outcome);
        return Ok(LockedDecision::new(answers));
    }

    let started = Instant::now();
    let locked = SqlTupleStore::lock_in(tx, &witness).await?;
    span.record(
        "lock_ms",
        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    );
    let clock = ClusterTime::read_in(tx)
        .await
        .map_err(DatabaseError::from)?;
    let now = WallTime::from_unix_nanos(clock.nanos());

    let engine = authorizer.engine();
    let mut moved = 0_usize;
    let mut under: BTreeSet<String> = BTreeSet::new();
    for (walked, held) in witness.tuples().zip(&locked) {
        let still = match held {
            Some(held) => held == walked && engine.tuple_grants_at(held, now)?,
            None => false,
        };
        if !still {
            moved += 1;
            under.insert(format!("{}#{}", walked.object.namespace, walked.relation));
        }
    }
    if moved > 0 {
        span.record("outcome", "moved");
        warn!(
            moved,
            witness = witness.len(),
            relations = ?under,
            "locked check: witness tuples moved before their locks were granted; the transaction is re-run"
        );
        return Err(AuthzError::LockedWitnessMoved {
            moved,
            witness: witness.len(),
        });
    }
    span.record("outcome", "allowed");
    Ok(LockedDecision::new(answers))
}

/// Each question's answer and the union of the allowed witnesses, walked through `tx`
/// without a lock.
async fn walk_locked(
    authorizer: &NativeEngine<SqlTupleStore>,
    tx: &mut SqlTransaction,
    questions: &LockedQuestions,
) -> Result<(Vec<bool>, Witness), AuthzError> {
    let source = authorizer.engine();
    let limits = EvalLimits {
        parallel_fanout: false,
        ..source.limits()
    };
    let engine = Engine::with_limits(
        SqlTransactionView::open(tx).await?,
        Arc::clone(source.schema()),
        limits,
    );
    let mut answers = Vec::with_capacity(questions.len());
    let mut witness = Witness::default();
    let mut reads = 0_usize;
    for (object, relation, subject) in questions.iter() {
        let question = info_span!("authz.locked_question", object = %object, relation = %relation, subject = %subject, allowed = Empty);
        let (found, used) = engine
            .witness_in(object, relation, subject, engine.store().scope())
            .instrument(question.clone())
            .await?;
        question.record("allowed", found.is_some());
        reads += used;
        answers.push(found.is_some());
        if let Some(found) = found {
            witness.absorb(found);
        }
    }
    Span::current().record("walk_reads", reads);
    Ok((answers, witness))
}

#[instrument(name = "authz.check_in", skip_all, fields(object = %object, relation = %relation, subject = %subject), err(Debug))]
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
async fn check_within(
    authorizer: &NativeEngine<SqlTupleStore>,
    tx: &mut SqlTransaction,
    object: ObjectRef,
    relation: RelationName,
    subject: SubjectRef,
    context: CaveatContext,
) -> Result<bool, AuthzError> {
    require_serializable(tx)?;
    let engine = rebind(authorizer, tx).await?;
    engine
        .check_in(object, relation, subject, context, engine.store().scope())
        .await
}

#[instrument(name = "authz.expand_in", skip_all, fields(object = %object, relation = %relation), err(Debug))]
async fn expand_within(
    authorizer: &NativeEngine<SqlTupleStore>,
    tx: &mut SqlTransaction,
    object: ObjectRef,
    relation: RelationName,
) -> Result<UsersetTree, AuthzError> {
    require_serializable(tx)?;
    let engine = rebind(authorizer, tx).await?;
    engine
        .expand_in(object, relation, engine.store().scope())
        .await
}

/// The authorizer's engine, without its set index, walking the tuples through `tx`.
async fn rebind<'t>(
    authorizer: &NativeEngine<SqlTupleStore>,
    tx: &'t mut SqlTransaction,
) -> Result<Engine<SqlTransactionView<'t>>, AuthzError> {
    let engine = authorizer.engine();
    Ok(Engine::with_limits(
        SqlTransactionView::open(tx).await?,
        Arc::clone(engine.schema()),
        engine.limits(),
    ))
}

/// Refuses `tx` unless it was begun at `expected`.
fn require_isolation(
    tx: &SqlTransaction,
    expected: IsolationLevel,
    refuse: impl FnOnce(String) -> AuthzError,
) -> Result<(), AuthzError> {
    match tx.isolation() {
        Some(level) if level == expected => Ok(()),
        other => {
            let found = other.map_or_else(
                || "an unstated isolation level".to_owned(),
                |level| level.as_sql().to_owned(),
            );
            warn!(found = %found, expected = %expected.as_sql(), "authorization decision refused: the caller's transaction is at the wrong isolation");
            Err(refuse(found))
        }
    }
}

fn require_read_committed(tx: &SqlTransaction) -> Result<(), AuthzError> {
    require_isolation(tx, IsolationLevel::ReadCommitted, |found| {
        AuthzError::LockedCheckIsolation { found }
    })
}

fn require_serializable(tx: &SqlTransaction) -> Result<(), AuthzError> {
    require_isolation(tx, IsolationLevel::Serializable, |found| {
        AuthzError::IsolationTooWeak { found }
    })
}
