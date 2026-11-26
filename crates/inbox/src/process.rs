//! [`process`] — one attempt at one delivery: the inbox row and the effect, in one transaction.

use chrono::{DateTime, Utc};
use fundamental_db::{
    ConstraintKind, DatabaseError, Filter, QueryExecutor as _, Repository as _,
    RepositoryTransaction as _,
};
use fundamental_sql::{ClusterTime, EffectTransaction, SqlPool, ViolatedConstraint};
use tokio::sync::Notify;
use tracing::{debug, warn};

use crate::attempt::Attempt;
use crate::dispatch::Dispatch;
use crate::inbox_row::InboxRow;
use crate::verdict::Verdict;
use crate::watermark::Watermark;
use crate::{EffectName, Inbound, InboxConfig, InboxError, ParkReason};
use fundamental_core::Error as _;
use fundamental_nats::PayloadDigest;

/// Applies `inbound` once for `binding`'s effect, or finds that it already was. `began` is
/// notified once the transaction holds a connection, which is where the effect's deadline starts.
///
/// A plain `INSERT`, not `ON CONFLICT DO NOTHING`: on YSQL the latter contends across distinct
/// keys under `SERIALIZABLE`. A unique violation then means the row is committed.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
pub(crate) async fn process<M, E>(
    dispatch: &Dispatch<M, E>,
    inbound: &Inbound<M>,
    digest: PayloadDigest,
    watermark: Watermark,
    began: &Notify,
) -> Verdict
where
    M: Send + Sync,
    E: fundamental_core::Error + From<DatabaseError> + Send + 'static,
{
    let Dispatch {
        pool,
        config,
        binding,
        step,
        ..
    } = dispatch;
    let step = *step;
    let effect = binding.effect();
    let outcome = pool
        .transaction(binding.isolation(), async move |tx| {
            began.notify_one();
            let now = ClusterTime::read_in(tx)
                .await
                .map_err(|error| Attempt::<E>::Store(DatabaseError::from(error)))?
                .to_utc();
            // A reading too old to rely on may predate deletes below a newer watermark; the
            // delivery waits for the listener's next reading rather than guess.
            if !watermark.trusted_at(now) {
                return Ok(Verdict::Failed {
                    code: unavailable_code(),
                    retryable: true,
                });
            }
            if let Some(refusal) = outside_window(config, inbound, now, watermark) {
                return Ok(refusal);
            }
            let row = InboxRow {
                effect: effect.as_str().to_owned(),
                event_id: inbound.event_id().as_uuid(),
                sweep_bucket: i32::from(inbound.event_id().bucket()),
                payload_digest: digest.as_bytes().to_vec(),
                stream: binding.stream().to_owned(),
                consumer: binding.consumer().to_owned(),
                stream_sequence: i64::try_from(inbound.delivery().stream_sequence)
                    .unwrap_or(i64::MAX),
                processed_at: now,
            };
            if let Err(error) = tx.insert(&row).await {
                return Err(if is_unique_violation(&error) {
                    Attempt::AlreadyRecorded
                } else {
                    Attempt::Store(error)
                });
            }
            step(&mut EffectTransaction::scope(tx), inbound)
                .await
                .map_err(Attempt::Effect)?;
            Ok(Verdict::Processed)
        })
        .await;
    match outcome {
        Ok(verdict) => verdict,
        Err(Attempt::AlreadyRecorded) => already_recorded(pool, effect, inbound, digest).await,
        Err(Attempt::Store(DatabaseError::CommitOutcomeUnknown { .. })) => Verdict::CommitUnknown,
        Err(error) => {
            debug!(effect = %effect, code = error.error_code(), error = %error, "the effect's transaction failed");
            Verdict::Failed {
                code: error.error_code().to_owned(),
                retryable: error.is_retryable(),
            }
        }
    }
}

/// The code a delivery carries when the inbox's own store could not answer.
fn unavailable_code() -> String {
    InboxError::Unavailable {
        cause: DatabaseError::internal(String::new()),
    }
    .error_code()
    .to_owned()
}

fn is_unique_violation(error: &DatabaseError) -> bool {
    ViolatedConstraint::of(error).is_some_and(|violated| violated.kind == ConstraintKind::Unique)
}

/// What a committed inbox row for this event says: the same payload is a redelivery, another
/// is a reused id.
async fn already_recorded<M>(
    pool: &SqlPool,
    effect: EffectName,
    inbound: &Inbound<M>,
    digest: PayloadDigest,
) -> Verdict {
    let query = pool
        .repository::<InboxRow>()
        .query()
        .pin_key(effect.as_str())
        .filter(Filter::eq(InboxRow::EVENT_ID, inbound.event_id().as_uuid()))
        .limit(1);
    match pool.query_executor::<InboxRow>().execute(&query).await {
        Ok(rows) => match rows.first() {
            Some(stored) if stored.payload_digest == digest.as_bytes() => Verdict::Duplicate,
            Some(_) => Verdict::Refused {
                reason: ParkReason::EventIdReused,
                detail: String::new(),
            },
            // The row that refused the insert is gone again — swept, or its writer's commit
            // was undone. Delivering again inserts it afresh or finds it.
            None => Verdict::Failed {
                code: unavailable_code(),
                retryable: true,
            },
        },
        Err(error) => {
            warn!(effect = %effect, %error, "the recorded inbox row could not be read; delivering again");
            Verdict::Failed {
                code: unavailable_code(),
                retryable: true,
            }
        }
    }
}

/// A refusal when the event is older than the acceptance window or the effect's watermark, or
/// implausibly far ahead of `now`.
fn outside_window<M>(
    config: &InboxConfig,
    inbound: &Inbound<M>,
    now: DateTime<Utc>,
    watermark: Watermark,
) -> Option<Verdict> {
    let occurred = inbound.event_id().occurred_at()?;
    let age = now.signed_duration_since(occurred);
    if age > config.acceptance_window() {
        return Some(Verdict::Refused {
            reason: ParkReason::BeyondHorizon,
            detail: format!(
                "occurred {} h ago; this listener applies events under {} h old",
                age.num_hours(),
                config.acceptance_window().num_hours()
            ),
        });
    }
    if let Some(covered_from) = watermark.covered_from
        && occurred < covered_from + config.max_skew()
    {
        return Some(Verdict::Refused {
            reason: ParkReason::BeyondHorizon,
            detail: format!("occurred at {occurred}; the ledger is complete from {covered_from}"),
        });
    }
    if -age > config.max_skew() {
        return Some(Verdict::Refused {
            reason: ParkReason::FromTheFuture,
            detail: format!(
                "occurred at {occurred}, {} s ahead of the database's clock",
                -age.num_seconds()
            ),
        });
    }
    None
}
