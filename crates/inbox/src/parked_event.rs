//! [`ParkedEvent`] — a refused delivery, as an operator reads it.

use chrono::{DateTime, Utc};
use fundamental_nats::EventId;

use crate::parked_row::ParkedRow;
use crate::{ParkReason, ParkedAddress};

/// A delivery the inbox refused to apply, kept until an operator resolves it or its window
/// passes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParkedEvent {
    /// Where the record is — also what `inbox:resolve` takes.
    pub address: ParkedAddress,
    /// The event's id, when it had a usable one.
    pub event_id: Option<EventId>,
    /// Why it was refused, or `None` for a reason this version does not know.
    pub reason: Option<ParkReason>,
    /// The error code of the last failure, when a failure parked it.
    pub error_code: Option<String>,
    /// Which attempt parked it.
    pub delivered: u64,
    /// When, by the database's clock.
    pub parked_at: DateTime<Utc>,
}

impl From<ParkedRow> for ParkedEvent {
    fn from(row: ParkedRow) -> Self {
        Self {
            address: ParkedAddress {
                effect: row.effect,
                stream: row.stream,
                stream_created_at: row.stream_created_at,
                stream_sequence: u64::try_from(row.stream_sequence).unwrap_or(0),
            },
            event_id: row.event_id.map(EventId::from_uuid),
            reason: row.reason.parse().ok(),
            error_code: row.error_code,
            delivered: u64::try_from(row.delivered).unwrap_or(0),
            parked_at: row.parked_at,
        }
    }
}
