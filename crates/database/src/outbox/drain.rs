//! [`drain_pending`]: claim one batch of due outbox rows and relay each.

use chrono::{DateTime, Utc};
use fundamental_core::AnyError;
use fundamental_core::model::Database;
use fundamental_core::outbox::DrainReport;
use tracing::{Span, info, instrument, warn};

use super::{OutboxClaimer, OutboxRow, Relay, RetryPolicy};

/// Claims up to `batch` rows due at `now` and relays each, reporting what the tick did.
///
/// The claim is exclusive and commits before any relay runs, so replicas get disjoint work,
/// no transaction is held across the relay's I/O, and a dead worker's rows return when its
/// lease expires. A row that fails is recorded ([`OutboxRow::note_failure`]) and deferred by
/// `policy` through [`OutboxClaimer::record_failure`], only while it still carries this
/// claim's lease, so one poison row cannot starve the queue. A row that cannot be decoded is
/// never relayed. [`Relay`] performs the effect and marks the row, so a crash never marks a
/// row whose effect did not commit.
///
/// # Errors
/// Returns [`AnyError`] only if the claim fails, in which case nothing was taken. Per-row
/// relay failures are recorded on the row and do not abort the batch.
#[instrument(skip_all, fields(batch, claimed = tracing::field::Empty, relayed = tracing::field::Empty, set_aside = tracing::field::Empty, skipped = tracing::field::Empty))]
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
pub async fn drain_pending<D, R, Claimer, Rel>(
    claimer: &Claimer,
    relay: &Rel,
    scope: &R::Scope,
    batch: u64,
    policy: RetryPolicy,
    now: DateTime<Utc>,
) -> Result<DrainReport, AnyError>
where
    D: Database,
    R: OutboxRow<D>,
    Claimer: OutboxClaimer<R, D>,
    Rel: Relay<R>,
{
    let leased_until = policy.lease_until(now);
    let claim = claimer.claim(scope, batch, now, leased_until).await?;
    let claimed = claim.rows.len();
    let span = Span::current();
    span.record("claimed", claimed);
    span.record("set_aside", claim.set_aside.total());
    span.record("skipped", claim.skipped);

    let mut relayed = 0usize;
    for mut row in claim.rows {
        match relay.relay(&row).await {
            Ok(()) => relayed += 1,
            Err(error) => {
                let code = error.get().error_code();
                row.note_failure(&format!("[{code}] {error}"));
                // Read after recording the failure, so the first failure waits one base delay.
                let msg_id = row.msg_id().into_owned();
                let retry_at = policy.retry_at(row.attempts(), &msg_id, now);
                row.defer_until(retry_at);
                warn!(
                    msg_id = %msg_id,
                    code,
                    attempts = row.attempts(),
                    %retry_at,
                    %error,
                    "outbox relay failed; deferred to its next attempt"
                );
                match claimer.record_failure(scope, &row, leased_until).await {
                    Ok(true) => {}
                    Ok(false) => info!(
                        msg_id = %msg_id,
                        relay_code = code,
                        "outbox row moved on while it was relayed; its failure was not written over it",
                    ),
                    // The row keeps its lease and is retried once that lapses; only the record of why is lost.
                    Err(update_error) => warn!(
                        msg_id = %msg_id,
                        code = update_error.get().error_code(),
                        relay_code = code,
                        %update_error,
                        "failed to record outbox relay failure; row stays claimable",
                    ),
                }
            }
        }
    }
    span.record("relayed", relayed);
    Ok(DrainReport {
        claimed,
        relayed,
        set_aside: claim.set_aside,
        skipped: claim.skipped,
    })
}
