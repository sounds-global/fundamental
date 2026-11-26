//! [`park`] — the durable record of a delivery the inbox refused.

use fundamental_db::{DatabaseError, IsolationLevel, RepositoryTransaction as _};
use fundamental_nats::{Delivery, EventId};
use fundamental_sql::{ClusterTime, SqlPool};

use crate::parked_row::ParkedRow;
use crate::{EffectName, InboxError, ParkReason};

/// Records that `delivery` was refused for `effect`, and why; recording it twice succeeds.
///
/// # Errors
///
/// [`InboxError::Unavailable`] or [`InboxError::TableMissing`] when the record could not be
/// written, in which case the delivery must not be terminated.
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
pub(crate) async fn park(
    pool: &SqlPool,
    effect: EffectName,
    stream_created_at: chrono::DateTime<chrono::Utc>,
    delivery: &Delivery,
    event_id: Option<EventId>,
    reason: ParkReason,
    code: Option<&str>,
) -> Result<(), InboxError> {
    pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
        let now = ClusterTime::read_in(tx)
            .await
            .map_err(DatabaseError::from)?
            .to_utc();
        tx.insert_if_absent(&ParkedRow {
            effect: effect.as_str().to_owned(),
            stream: delivery.stream.clone(),
            stream_created_at,
            stream_sequence: i64::try_from(delivery.stream_sequence).unwrap_or(i64::MAX),
            event_id: event_id.map(|id| id.as_uuid()),
            reason: reason.as_str().to_owned(),
            error_code: code.map(str::to_owned),
            delivered: i64::try_from(delivery.delivered).unwrap_or(i64::MAX),
            parked_at: now,
        })
        .await?;
        Ok::<_, DatabaseError>(())
    })
    .await
    .map_err(InboxError::from_store)
}
